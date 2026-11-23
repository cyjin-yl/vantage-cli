use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use lenovo_laptop_control_core::{ApplyMode, LenovoLaptopControlError, Result};
use roxmltree::{Document, Node};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{CommandResult, UpdateCatalogOs, structured_output};

const LENOVO_CDN: &str = "https://download.lenovo.com/";
const MAX_XML_BYTES: usize = 2 * 1024 * 1024;
const DESCRIPTOR_WORKERS: usize = 6;

#[derive(Serialize)]
struct Catalog {
    machine_type: String,
    os: &'static str,
    catalog_url: String,
    packages: Vec<Package>,
}

#[derive(Serialize)]
struct Package {
    id: String,
    title: String,
    version: String,
    category: String,
    release_date: Option<String>,
    descriptor_url: String,
    descriptor_sha256: String,
    /// `Files/File` payload entries the descriptor declares, verbatim.
    files: Vec<PackageFile>,
    /// Device bindings the descriptor declares (`DetectVersion/_PnPID` and
    /// `DetectInstall/_Driver/HardwareID`).
    bindings: Vec<String>,
    /// Whether the package binds to hardware this machine has.
    applies: Applicability,
}

#[derive(Serialize)]
struct PackageFile {
    name: String,
    sha256: String,
    size: u64,
}

/// Whether a catalog package binds to hardware this machine actually has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Applicability {
    /// A declared binding matches a present device.
    Applies,
    /// The package declares bindings and none of them is present.
    DoesNotApply,
    /// The package declares no device binding (system-wide, firmware, or
    /// manual packages); the catalog cannot say whether it applies.
    Unknown,
}

pub(super) fn check(
    mtm: &str,
    os: UpdateCatalogOs,
    inventory: Option<&dyn lenovo_laptop_control_hal::HardwareInventoryControl>,
    applicable_only: bool,
) -> CommandResult {
    let mut catalog = load(mtm, os, fetch_xml)?;
    let mut inventory_ids = Vec::new();
    if let Some(inventory) = inventory {
        inventory_ids = inventory.present_hardware_ids()?;
        apply_inventory(&mut catalog, &inventory_ids);
    }
    if applicable_only {
        catalog
            .packages
            .retain(|package| package.applies == Applicability::Applies);
    }
    let mut human = format!(
        "{} package(s) in Lenovo's public {} catalog for {}\n",
        catalog.packages.len(),
        catalog.os,
        catalog.machine_type
    );
    if !inventory_ids.is_empty() {
        let _ = writeln!(
            human,
            "compared against {} present hardware ID(s) on this machine",
            inventory_ids.len()
        );
    }
    for package in &catalog.packages {
        let _ = writeln!(
            human,
            "  {} {} [{}] ({}) {}",
            package.title,
            package.version,
            package.id,
            package.category,
            match package.applies {
                Applicability::Applies => "applies to this machine",
                Applicability::DoesNotApply => "no matching device on this machine",
                Applicability::Unknown => "no device binding declared",
            }
        );
    }
    structured_output(&catalog, human)
}

#[derive(Serialize)]
struct DownloadReport {
    machine_type: String,
    os: &'static str,
    catalog_url: String,
    mode: &'static str,
    packages: Vec<DownloadedPackage>,
}

#[derive(Serialize)]
struct DownloadedPackage {
    id: String,
    title: String,
    destination: String,
    /// Whether the package binds to hardware present on this machine.
    applies: Applicability,
    files: Vec<DownloadedPackageFile>,
}

#[derive(Serialize)]
struct DownloadedPackageFile {
    path: String,
    size: u64,
    sha256: String,
}

/// `update download`: resolve each selector against the public catalog and
/// fetch every payload file the descriptor declares, verifying the published
/// SHA-256 and size before accepting it. Each package lands in its own
/// directory so same-named files cannot collide. `DryRun` reports the plan
/// without writing anything; catalog reads still happen.
pub(super) fn download(
    selectors: &[String],
    destination: &Path,
    mtm: Option<&str>,
    os: Option<UpdateCatalogOs>,
    inventory: Option<&dyn lenovo_laptop_control_hal::HardwareInventoryControl>,
    mode: ApplyMode,
) -> CommandResult {
    let os = match (os, mtm) {
        (Some(os), _) => os,
        (None, Some(_)) => UpdateCatalogOs::Win11,
        (None, None) => detect_os(),
    };
    let detected_mtm;
    let mtm = match mtm {
        Some(mtm) => mtm,
        None => {
            detected_mtm = detect_mtm()?;
            &detected_mtm
        }
    };
    let mut catalog = load(mtm, os, fetch_xml)?;
    if let Some(inventory) = inventory {
        let ids = inventory.present_hardware_ids()?;
        apply_inventory(&mut catalog, &ids);
    }
    let selected = selectors
        .iter()
        .map(|selector| find_package(&catalog, selector))
        .collect::<Result<Vec<_>>>()?;
    let mut packages = Vec::with_capacity(selectors.len());
    let mut human = String::new();
    for package in selected {
        if package.files.is_empty() {
            return Err(LenovoLaptopControlError::Unsupported {
                feature: format!("update.download: {} declares no payload files", package.id),
            });
        }
        validate_component(&package.id, &package.descriptor_url)?;
        let directory = destination.join(&package.id);
        for file in &package.files {
            validate_component(&file.name, &package.descriptor_url)?;
        }
        if mode == ApplyMode::Commit {
            std::fs::create_dir_all(&directory)?;
            if !std::fs::symlink_metadata(&directory)?.file_type().is_dir() {
                return Err(catalog_error(
                    &package.descriptor_url,
                    format!("{} is not a package directory", directory.display()),
                ));
            }
        }
        let mut files = Vec::with_capacity(package.files.len());
        for file in &package.files {
            let path = directory.join(&file.name);
            if mode == ApplyMode::Commit {
                save_file(&payload_url(package, &file.name), &path, file, fetch_file)?;
            } else {
                let _ = writeln!(
                    human,
                    "would download {} ({} bytes)",
                    path.display(),
                    file.size
                );
            }
            files.push(DownloadedPackageFile {
                path: path.to_string_lossy().into_owned(),
                size: file.size,
                sha256: file.sha256.to_ascii_lowercase(),
            });
        }
        if mode == ApplyMode::Commit {
            let _ = writeln!(
                human,
                "verified {} {} ({} file(s)) in {} [{}]",
                package.title,
                package.id,
                files.len(),
                directory.display(),
                match package.applies {
                    Applicability::Applies => "applies to this machine",
                    Applicability::DoesNotApply => "no matching device on this machine",
                    Applicability::Unknown => "no device binding declared",
                }
            );
        }
        packages.push(DownloadedPackage {
            id: package.id.clone(),
            title: package.title.clone(),
            destination: directory.to_string_lossy().into_owned(),
            applies: package.applies,
            files,
        });
    }
    structured_output(
        &DownloadReport {
            machine_type: catalog.machine_type,
            os: catalog.os,
            catalog_url: catalog.catalog_url,
            mode: match mode {
                ApplyMode::DryRun => "dry_run",
                ApplyMode::Commit => "commit",
            },
            packages,
        },
        human,
    )
}

fn validate_component(name: &str, url: &str) -> Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved
        || name.is_empty()
        || name.starts_with('.')
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(catalog_error(
            url,
            format!("invalid download name {name:?}"),
        ));
    }
    Ok(())
}

fn save_file(
    url: &str,
    path: &Path,
    file: &PackageFile,
    fetch: impl FnOnce(&str, &Path, u64) -> Result<()>,
) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => return verify_file(path, file),
        Ok(_) => {
            return Err(catalog_error(
                url,
                format!("{} is not a regular file", path.display()),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let directory = path
        .parent()
        .ok_or_else(|| catalog_error(url, "download path has no parent"))?;
    let temporary = tempfile::NamedTempFile::new_in(directory)?.into_temp_path();
    fetch(url, &temporary, file.size)?;
    verify_file(&temporary, file)?;
    match temporary.persist_noclobber(path) {
        Ok(()) => Ok(()),
        Err(error)
            if error.error.kind() == std::io::ErrorKind::AlreadyExists
                && std::fs::symlink_metadata(path)?.is_file() =>
        {
            verify_file(path, file)
        }
        Err(error) => Err(error.error.into()),
    }
}

fn verify_file(path: &Path, file: &PackageFile) -> Result<()> {
    let size = std::fs::metadata(path)?.len();
    if size != file.size {
        return Err(LenovoLaptopControlError::VerifyMismatch {
            requested: format!("{} size {}", file.name, file.size),
            actual: size.to_string(),
        });
    }
    let actual = file_sha256(path)?;
    if !actual.eq_ignore_ascii_case(&file.sha256) {
        return Err(LenovoLaptopControlError::VerifyMismatch {
            requested: format!("{} SHA-256 {}", file.name, file.sha256),
            actual,
        });
    }
    Ok(())
}

/// Prefer package IDs, then accept a title substring when it is unambiguous.
fn find_package<'a>(catalog: &'a Catalog, selector: &str) -> Result<&'a Package> {
    let mut ids = catalog
        .packages
        .iter()
        .filter(|package| package.id.eq_ignore_ascii_case(selector));
    if let Some(package) = ids.next() {
        if ids.next().is_some() {
            return Err(LenovoLaptopControlError::InvalidArgument {
                detail: format!("package ID {selector:?} is ambiguous"),
            });
        }
        return Ok(package);
    }
    let needle = selector.to_ascii_lowercase();
    let mut title_matches = catalog
        .packages
        .iter()
        .filter(|package| package.title.to_ascii_lowercase().contains(&needle));
    match (title_matches.next(), title_matches.next()) {
        (Some(package), None) => Ok(package),
        (Some(_), Some(_)) => Err(LenovoLaptopControlError::InvalidArgument {
            detail: format!(
                "{selector:?} matches more than one catalog package; quote the package id"
            ),
        }),
        _ => Err(LenovoLaptopControlError::InvalidArgument {
            detail: format!(
                "{selector:?} is not in the {} catalog; run `vantage update check --mtm {}`",
                catalog.machine_type, catalog.machine_type
            ),
        }),
    }
}

/// Payload URL: the descriptor's directory plus the declared file name.
fn payload_url(package: &Package, name: &str) -> String {
    match package.descriptor_url.rsplit_once('/') {
        Some((directory, _)) => format!("{directory}/{name}"),
        None => format!("{}/{name}", package.descriptor_url),
    }
}

fn load(
    mtm: &str,
    os: UpdateCatalogOs,
    fetch: impl Fn(&str) -> Result<Vec<u8>> + Sync,
) -> Result<Catalog> {
    if mtm.len() < 4 || !mtm.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(LenovoLaptopControlError::InvalidArgument {
            detail:
                "--mtm requires a four-character Lenovo machine type or a full alphanumeric MTM"
                    .into(),
        });
    }
    let machine_type = mtm[..4].to_ascii_uppercase();
    let os = match os {
        UpdateCatalogOs::Win10 => "win10",
        UpdateCatalogOs::Win11 => "win11",
    };
    let catalog_url = format!("{LENOVO_CDN}catalog/{machine_type}_{os}.xml");
    let bytes = fetch(&catalog_url)?;
    let document = parse_xml(&bytes, &catalog_url)?;
    let root = document.root_element();
    if !root.has_tag_name("packages") {
        return Err(catalog_error(
            &catalog_url,
            "expected a packages root element",
        ));
    }
    let entries: Vec<_> = root
        .children()
        .filter(|node| node.has_tag_name("package"))
        .collect();
    let count = entries.len();
    if root.attribute("count").and_then(|value| value.parse().ok()) != Some(count) {
        return Err(catalog_error(
            &catalog_url,
            "package count does not match the index",
        ));
    }
    let packages = std::thread::scope(|scope| {
        let fetch = &fetch;
        let catalog_url = &catalog_url;
        let workers: Vec<_> = entries
            .chunks(count.div_ceil(DESCRIPTOR_WORKERS).max(1))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|entry| read_package(*entry, catalog_url, fetch))
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect();
        let mut packages = Vec::with_capacity(count);
        for worker in workers {
            packages.extend(
                worker
                    .join()
                    .map_err(|_| catalog_error(catalog_url, "descriptor worker failed"))??,
            );
        }
        Ok::<_, LenovoLaptopControlError>(packages)
    })?;
    Ok(Catalog {
        machine_type,
        os,
        catalog_url,
        packages,
    })
}

fn read_package(
    entry: Node<'_, '_>,
    catalog_url: &str,
    fetch: &impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Package> {
    let location = child(entry, "location").and_then(text).unwrap_or_default();
    if !location.starts_with(LENOVO_CDN) || !location.ends_with(".xml") {
        return Err(catalog_error(
            catalog_url,
            "expected a Lenovo CDN descriptor URL",
        ));
    }
    let checksum = child(entry, "checksum")
        .filter(|node| {
            node.attribute("type")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("sha256"))
        })
        .and_then(text)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| catalog_error(location, "missing or invalid descriptor SHA-256"))?;
    let descriptor = fetch(location)?;
    let actual = format!("{:x}", Sha256::digest(&descriptor));
    if !actual.eq_ignore_ascii_case(checksum) {
        return Err(LenovoLaptopControlError::VerifyMismatch {
            requested: format!("{location} SHA-256 {checksum}"),
            actual,
        });
    }
    let package_xml = parse_xml(&descriptor, location)?;
    let package = package_xml.root_element();
    if !package.has_tag_name("Package") {
        return Err(catalog_error(location, "expected a Package root element"));
    }
    let attribute = |name| {
        package
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| catalog_error(location, format!("missing package {name}")))
    };
    let id = attribute("id")?;
    let title = child(package, "Title").and_then(|title| {
        let default = title.attribute("default").unwrap_or("EN");
        let descriptions = title.children().filter(|node| node.has_tag_name("Desc"));
        descriptions
            .clone()
            .find(|node| node.attribute("id") == Some(default))
            .and_then(text)
            .or_else(|| descriptions.filter_map(text).next())
    });
    Ok(Package {
        id: id.into(),
        title: title
            .or_else(|| package.attribute("name"))
            .unwrap_or(id)
            .into(),
        version: attribute("version")?.into(),
        category: child(entry, "category")
            .and_then(text)
            .unwrap_or_default()
            .into(),
        release_date: child(package, "ReleaseDate")
            .and_then(text)
            .map(str::to_owned),
        descriptor_url: location.into(),
        descriptor_sha256: actual,
        files: package_files(&package_xml, location)?,
        bindings: package_bindings(&package_xml),
        applies: Applicability::Unknown,
    })
}

/// The device bindings a descriptor declares: `DetectVersion/_PnPID` entries
/// and the `DetectInstall/_Driver/HardwareID` list, verbatim.
fn package_bindings(document: &Document<'_>) -> Vec<String> {
    let mut bindings: Vec<String> = document
        .descendants()
        .filter(|node| node.has_tag_name("_PnPID") || node.has_tag_name("HardwareID"))
        .filter_map(|node| node.text())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    bindings.sort();
    bindings.dedup();
    bindings
}

/// Compare a package's declared bindings against the machine's present
/// hardware IDs. A trailing `*` in a binding is a prefix match, as PnP
/// hardware IDs use. A package without bindings stays `Unknown`: the catalog
/// cannot say whether a system-wide or manual package is needed.
fn applicability(bindings: &[String], present: &[String]) -> Applicability {
    if bindings.is_empty() {
        return Applicability::Unknown;
    }
    let matches = bindings.iter().any(|binding| {
        present.iter().any(|id| match binding.strip_suffix('*') {
            Some(prefix) => {
                id.len() >= prefix.len() && id[..prefix.len()].eq_ignore_ascii_case(prefix)
            }
            None => id.eq_ignore_ascii_case(binding),
        })
    });
    if matches {
        Applicability::Applies
    } else {
        Applicability::DoesNotApply
    }
}

/// Annotate every package with whether it binds to hardware present here.
fn apply_inventory(catalog: &mut Catalog, present: &[String]) {
    for package in &mut catalog.packages {
        package.applies = applicability(&package.bindings, present);
    }
}

/// The `Files/File` payload list of one descriptor: `{Name,CRC,Size}` triples
/// under the descriptor's `<Files>` block. A package without a file list is
/// admitted (some metadata-only entries carry none); a file without a hash or
/// size is rejected — it cannot be verified.
fn package_files(document: &Document<'_>, url: &str) -> Result<Vec<PackageFile>> {
    let files = document
        .descendants()
        .find(|node| node.has_tag_name("Files"))
        .map(|files| {
            files
                .descendants()
                .filter(|node| node.has_tag_name("File"))
                .map(|file| {
                    let name = child(file, "Name")
                        .and_then(text)
                        .ok_or_else(|| catalog_error(url, "file has no Name"))?;
                    let sha256 = child(file, "CRC")
                        .and_then(text)
                        .filter(|hash| {
                            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                        .ok_or_else(|| catalog_error(url, format!("file {name} has no SHA-256")))?;
                    let size = child(file, "Size")
                        .and_then(text)
                        .and_then(|raw| raw.parse::<u64>().ok())
                        .ok_or_else(|| catalog_error(url, format!("file {name} has no Size")))?;
                    Ok(PackageFile {
                        name: name.into(),
                        sha256: sha256.into(),
                        size,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(files)
}

fn child<'a, 'input>(node: Node<'a, 'input>, tag: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|node| node.has_tag_name(tag))
}

fn text<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    node.text().map(str::trim).filter(|value| !value.is_empty())
}

fn parse_xml<'a>(bytes: &'a [u8], url: &str) -> Result<Document<'a>> {
    let xml = std::str::from_utf8(bytes).map_err(|error| catalog_error(url, error))?;
    Document::parse(xml.trim_start_matches('\u{feff}')).map_err(|error| catalog_error(url, error))
}

/// This machine's MTM from SMBIOS data surfaced by Windows or Linux.
#[cfg(windows)]
fn detect_mtm() -> Result<String> {
    let output = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\HARDWARE\DESCRIPTION\System\BIOS",
            "/v",
            "SystemProductName",
        ])
        .output()
        .map_err(|error| {
            catalog_error(
                "machine identity",
                format!("could not run reg.exe: {error}"),
            )
        })?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| {
            line.split("REG_SZ")
                .nth(1)
                .map(str::trim)
                .map(str::to_owned)
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| catalog_error("machine identity", "SystemProductName is empty; pass --mtm"))
}

#[cfg(not(windows))]
fn detect_mtm() -> Result<String> {
    const DMI: &str = "/sys/class/dmi/id/product_name";
    std::fs::read_to_string(DMI)
        .map_err(|error| {
            catalog_error(
                "machine identity",
                format!("reading {DMI}: {error}; pass --mtm"),
            )
        })
        .and_then(|text| {
            let text = text.trim().to_string();
            if text.is_empty() {
                Err(catalog_error(
                    "machine identity",
                    format!("{DMI} is empty; pass --mtm"),
                ))
            } else {
                Ok(text)
            }
        })
}

/// The OS catalog key for this machine's Windows build. Non-Windows hosts
/// default to `win11` — the catalog payload is Windows software and `--os`
/// selects a different slice.
#[cfg(windows)]
fn detect_os() -> UpdateCatalogOs {
    let output = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "/v",
            "CurrentBuildNumber",
        ])
        .output();
    let Ok(output) = output else {
        return UpdateCatalogOs::Win11;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let build = text.lines().find_map(|line| {
        line.split("REG_SZ")
            .nth(1)
            .and_then(|v| v.trim().parse::<u32>().ok())
    });
    match build {
        Some(build) if build >= 22000 => UpdateCatalogOs::Win11,
        Some(_) => UpdateCatalogOs::Win10,
        None => UpdateCatalogOs::Win11,
    }
}

#[cfg(not(windows))]
fn detect_os() -> UpdateCatalogOs {
    UpdateCatalogOs::Win11
}

/// Stream one payload into the temporary file owned by `save_file`.
fn fetch_file(url: &str, destination: &Path, size: u64) -> Result<()> {
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let status = Command::new(curl)
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--globoff",
            "--proto",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "600",
            "--speed-limit",
            "1024",
            "--speed-time",
            "30",
            "--max-filesize",
        ])
        .arg(size.to_string())
        .arg("--output")
        .arg(destination)
        .args(["--write-out", "%{http_code}", "--", url])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| catalog_error(url, format!("could not run {curl}: {error}")))?;
    if !status.status.success() || status.stdout != b"200" {
        return Err(catalog_error(
            url,
            format!(
                "{curl} returned {} (HTTP {}): {}",
                status.status,
                String::from_utf8_lossy(&status.stdout),
                String::from_utf8_lossy(&status.stderr).trim()
            ),
        ));
    }
    Ok(())
}

/// SHA-256 of a downloaded file, streamed.
fn file_sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(LenovoLaptopControlError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(LenovoLaptopControlError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Default `update download` root: the user cache, `vantage/updates`.
#[cfg(windows)]
pub(super) fn default_download_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("vantage")
        .join("updates")
}

#[cfg(not(windows))]
pub(super) fn default_download_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("vantage")
        .join("updates")
}

fn catalog_error(url: &str, detail: impl std::fmt::Display) -> LenovoLaptopControlError {
    LenovoLaptopControlError::ChannelUnavailable {
        channel: format!("Lenovo public catalog {url}: {detail}"),
    }
}

fn fetch_xml(url: &str) -> Result<Vec<u8>> {
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let output = Command::new(curl)
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--globoff",
            "--proto",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "30",
            "--retry",
            "1",
            "--max-filesize",
        ])
        .arg(MAX_XML_BYTES.to_string())
        .args(["--write-out", "%{http_code}", "--", url])
        .output()
        .map_err(|error| catalog_error(url, format!("could not run {curl}: {error}")))?;
    let (body, status) = output
        .stdout
        .split_at(output.stdout.len().saturating_sub(3));
    if status == b"404" && url.starts_with("https://download.lenovo.com/catalog/") {
        return Err(catalog_error(
            url,
            "Lenovo has no public catalog for this machine type and OS (HTTP 404)",
        ));
    }
    if !output.status.success() || status != b"200" {
        return Err(catalog_error(
            url,
            format!(
                "{curl} returned {} (HTTP {}) {}",
                output.status,
                String::from_utf8_lossy(status),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    if body.len() > MAX_XML_BYTES {
        return Err(catalog_error(url, "XML response exceeds size limit"));
    }
    let mut bytes = output.stdout;
    bytes.truncate(bytes.len() - 3);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_payload_files_are_verified_before_publication() {
        let payload = b"driver";
        let sha256 = format!("{:x}", Sha256::digest(payload));
        let xml = format!(
            "<Package><Files><Installer><File><Name>driver.exe</Name><CRC>{sha256}</CRC><Size>6</Size></File></Installer></Files></Package>"
        );
        let document = Document::parse(xml.as_str()).unwrap();
        let files = package_files(&document, "descriptor").unwrap();
        let [file] = files.as_slice() else {
            panic!("expected one installer file")
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(&file.name);
        let error = save_file("payload", &path, file, |_, temporary, _| {
            std::fs::write(temporary, b"broken")?;
            Ok(())
        });
        assert!(matches!(
            error,
            Err(LenovoLaptopControlError::VerifyMismatch { .. })
        ));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        save_file("payload", &path, file, |_, temporary, _| {
            std::fs::write(temporary, payload)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        save_file("payload", &path, file, |_, _, _| {
            panic!("already verified file was fetched again")
        })
        .unwrap();
    }

    #[test]
    fn bindings_match_present_devices_with_prefix_wildcards() {
        let bindings = vec![
            "PCI\\VEN_8086&DEV_A3BA".to_string(),
            "USB\\VID_0BDA&PID_8153*".to_string(),
        ];
        assert_eq!(
            applicability(&bindings, &["pci\\ven_8086&dev_a3ba".to_string()]),
            Applicability::Applies
        );
        assert_eq!(
            applicability(&bindings, &["USB\\VID_0BDA&PID_8153".to_string()]),
            Applicability::Applies
        );
        assert_eq!(
            applicability(&bindings, &["PCI\\VEN_10DE&DEV_1EB8".to_string()]),
            Applicability::DoesNotApply
        );
        assert_eq!(
            applicability(&[], &["PCI\\VEN_10DE&DEV_1EB8".to_string()]),
            Applicability::Unknown
        );
    }

    #[test]
    fn descriptor_bindings_are_collected() {
        let xml = br#"<Package><DetectVersion type="Inf"><_PnPID><![CDATA[PCI\VEN_8086&DEV_A3BA]]></_PnPID></DetectVersion><DetectInstall><_Driver><HardwareID><![CDATA[PCI\VEN_8086&DEV_A3BA]]></HardwareID></_Driver></DetectInstall></Package>"#;
        let document = Document::parse(std::str::from_utf8(xml).unwrap()).unwrap();
        let bindings = package_bindings(&document);
        assert_eq!(bindings, vec!["PCI\\VEN_8086&DEV_A3BA".to_string()]);
    }

    #[test]
    fn full_mtm_selects_catalog_and_binds_descriptor_bytes() {
        let index_url = "https://download.lenovo.com/catalog/21CB_win10.xml";
        let descriptor_url = "https://download.lenovo.com/pccbbs/mobiles/sample.xml";
        let descriptor = br#"<Package id="sample" version="2.0"><Title default="EN"><Desc id="DE">Treiber</Desc><Desc id="EN">Audio &amp; Bluetooth</Desc></Title></Package>"#;
        let hash = format!("{:x}", Sha256::digest(descriptor));
        let index = format!(
            r#"<packages count="1"><package><location>{descriptor_url}</location><category>Audio</category><checksum type="sha256">{hash}</checksum></package></packages>"#
        );
        let report = load("21cb0001us", UpdateCatalogOs::Win10, |url| match url {
            url if url == index_url => Ok(index.as_bytes().to_vec()),
            url if url == descriptor_url => Ok(descriptor.to_vec()),
            url => panic!("unexpected fetch {url}"),
        })
        .unwrap();
        assert_eq!(report.packages[0].title, "Audio & Bluetooth");
        assert_eq!(report.packages[0].version, "2.0");

        let error = load("21CB", UpdateCatalogOs::Win10, |url| match url {
            url if url == index_url => Ok(index.as_bytes().to_vec()),
            url if url == descriptor_url => Ok(b"<Package id='changed' version='3'/>".to_vec()),
            url => panic!("unexpected fetch {url}"),
        });
        assert!(matches!(
            error,
            Err(LenovoLaptopControlError::VerifyMismatch { .. })
        ));
    }
}
