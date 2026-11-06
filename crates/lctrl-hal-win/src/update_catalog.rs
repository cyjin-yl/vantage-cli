//! Read-only evaluation of the Lenovo UDC (Unified Device Catalog) config the
//! machine already synced into `HKLM\SOFTWARE\Lenovo\UDC`.
//!
//! UDC is an mTLS applicability+manifest pipeline (see
//! `docs/evidence/update-contract-recon-20261104.md`). The live, server-driven
//! check needs the device client certificate, which this backend does not
//! present. What *is* available read-only is the signed config the service
//! last wrote to the registry: `Packages\LastUpdateConfig` holds the full
//! package list plus each package's `applicability_filter`, and
//! `Applicability\Tags` holds the device facts the server filters on. Evaluating
//! those two locally yields the set of packages the catalog offers *this*
//! machine — no network, no writes, no cert.
//!
//! Download/install stay unsupported: the contract for fetching and flashing a
//! package (`package_manifest_url` + signature + `package_handler`) is not a
//! read operation and is intentionally not implemented here.

use lctrl_core::{LctrlError, Result, UpdateCheckReport};

/// What the locally synced UDC catalogue proves: the service wrote a
/// parseable `LastUpdateConfig` and its sync provenance value.
///
/// This is the read-only precondition behind the `update.catalog.local`
/// capability id — it answers "does a synced catalogue exist", never "which
/// packages apply to this machine" (`check_updates_impl` answers that). One
/// registry read per value plus a JSON parse: no network, no package
/// evaluation, no install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogSync {
    /// `UpdateCheckDateTime` — the provenance of the sync. A sibling registry
    /// value, not a JSON field, so it is absent when the service wrote only
    /// the config.
    pub last_sync: Option<String>,
    /// How many packages the synced config lists, before applicability.
    pub packages: usize,
    /// `ConfigUpdateRequired=1` in the synced JSON: the cached config is
    /// marked stale by the service.
    pub stale: bool,
}

#[cfg(any(windows, test))]
mod catalog {
    use lctrl_core::CatalogPackage;
    use serde::Deserialize;
    use std::collections::BTreeMap;

    /// Registry subtree the UDC service publishes.
    pub(crate) const UDC_KEY: &str = r"SOFTWARE\Lenovo\UDC";
    pub(crate) const UDC_PACKAGES_KEY: &str = r"SOFTWARE\Lenovo\UDC\Packages";
    pub(crate) const UDC_TAGS_KEY: &str = r"SOFTWARE\Lenovo\UDC\Applicability\Tags";

    /// The synced catalog config, deserialized from `LastUpdateConfig` JSON.
    #[derive(Debug, Deserialize)]
    pub(crate) struct UpdateConfig {
        #[serde(default)]
        pub(crate) packages: Vec<RawPackage>,
        /// `ConfigUpdateRequired=1` marks the cached config stale.
        #[serde(default)]
        pub(crate) config_update_required: Option<u32>,
    }

    #[derive(Debug, Deserialize)]
    pub(crate) struct RawPackage {
        /// UDC's stable package identifier; the download cache directory is
        /// named after it.
        #[serde(default)]
        pub(crate) package_id: Option<String>,
        #[serde(default)]
        pub(crate) package_name: Option<String>,
        #[serde(default)]
        pub(crate) package_version: Option<String>,
        #[serde(default)]
        pub(crate) package_type: Option<String>,
        #[serde(default)]
        pub(crate) package_update_type: Option<String>,
        /// UDC serializes `package_size` as a quoted decimal ("2787227"), not
        /// a JSON number. Accept either shape.
        #[serde(default, deserialize_with = "deserialize_u64_flexible")]
        pub(crate) package_size: Option<u64>,
        #[serde(default)]
        pub(crate) package_manifest_url: Option<String>,
        #[serde(default)]
        pub(crate) package_url: Option<String>,
        #[serde(default)]
        pub(crate) package_file_hash: Option<String>,
        #[serde(default)]
        pub(crate) package_handler: Option<String>,
        /// Conjunction of applicability terms; empty/`[{"default":true}]` = apply to all.
        #[serde(default)]
        pub(crate) applicability_filter: Vec<ApplicabilityTerm>,
        /// Orgs this package applies to; the device's `registration-account` must
        /// be a member unless `applyToAllOrgs` is set.
        #[serde(default, rename = "applicableOrgs")]
        pub(crate) applicable_orgs: Vec<String>,
        #[serde(default, rename = "applyToAllOrgs")]
        pub(crate) apply_to_all_orgs: bool,
    }

    #[derive(Debug, Deserialize)]
    pub(crate) struct ApplicabilityTerm {
        #[serde(default)]
        pub(crate) mfr: Option<String>,
        #[serde(default)]
        pub(crate) mtm: Option<String>,
        #[serde(default)]
        pub(crate) os: Option<String>,
        #[serde(default)]
        pub(crate) os_bitness: Option<String>,
        /// Semver range like `<25.2.0.0` or `>=2.0`; bare version means `>=`.
        #[serde(default)]
        pub(crate) version: Option<String>,
        /// `|`-separated `Key|value` terms evaluated against `Applicability\Tags`.
        #[serde(default)]
        pub(crate) tag: Option<String>,
    }

    /// The device facts a filter is evaluated against.
    pub struct DeviceApplicability<'a> {
        /// `registration-account` (e.g. `LenovoAutoclaimed`).
        pub org: &'a str,
        /// Machine type model (e.g. `YX0JGHX9` / `21VG`), uppercase-insensitive match.
        pub mtm: &'a str,
        /// Device tags from `Applicability\Tags` (exact `Key` -> `value`).
        pub tags: &'a BTreeMap<String, String>,
    }

    /// Evaluate a parsed config against device facts, returning the offered set.
    /// Pure — the registry read happens in `check_updates`.
    pub fn evaluate(
        config: &UpdateConfig,
        device: &DeviceApplicability<'_>,
    ) -> Vec<CatalogPackage> {
        config
            .packages
            .iter()
            .filter(|package| package_applies(package, device))
            .map(to_catalog_package)
            .collect()
    }

    /// Project one catalogue entry, keeping only what a caller can act on.
    pub(crate) fn to_catalog_package(package: &RawPackage) -> CatalogPackage {
        CatalogPackage {
            name: package.package_name.clone().unwrap_or_default(),
            version: package.package_version.clone().unwrap_or_default(),
            package_type: package.package_type.clone(),
            update_type: package.package_update_type.clone(),
            size_bytes: package.package_size,
            manifest_url: package.package_manifest_url.clone(),
            download_url: package.package_url.clone(),
            file_hash: package.package_file_hash.clone(),
            handler: package.package_handler.clone(),
        }
    }

    pub(crate) fn package_applies(package: &RawPackage, device: &DeviceApplicability<'_>) -> bool {
        // Org gate: `applyToAllOrgs` short-circuits; otherwise the device's
        // registration-account must be listed.
        if !package.apply_to_all_orgs
            && !package.applicable_orgs.is_empty()
            && !package
                .applicable_orgs
                .iter()
                .any(|org| org.eq_ignore_ascii_case(device.org))
        {
            return false;
        }
        // Every listed term must hold; an absent filter list means "all devices".
        package
            .applicability_filter
            .iter()
            .all(|term| term_applies(term, device))
    }

    pub(crate) fn term_applies(term: &ApplicabilityTerm, device: &DeviceApplicability<'_>) -> bool {
        if let Some(mfr) = &term.mfr {
            if !mfr.eq_ignore_ascii_case("lenovo") {
                return false;
            }
        }
        if let Some(mtm) = &term.mtm {
            // Sentinel/wildcard values ("NotApplicable*", empty, "*") always match.
            let sentinel = mtm.is_empty()
                || mtm == "*"
                || mtm.to_ascii_lowercase().starts_with("notapplicable");
            if !sentinel && !mtm.eq_ignore_ascii_case(device.mtm) {
                return false;
            }
        }
        if let Some(os) = &term.os {
            if !os.eq_ignore_ascii_case("windows") {
                return false;
            }
        }
        if let Some(bits) = &term.os_bitness {
            if !(bits == "64" || bits == "*" || bits.is_empty()) {
                return false;
            }
        }
        if let Some(range) = &term.version {
            // The catalog's `version` term bounds the *installed* app version we
            // would be updating to; with no installed version to compare we treat a
            // range as "offers an update" (a check, not a compatibility proof).
            if parse_range(range).is_none() {
                return false; // unparseable bound -> cannot evaluate, exclude
            }
        }
        if let Some(tag) = &term.tag {
            if !tag_applies(tag, device.tags) {
                return false;
            }
        }
        true
    }

    /// `tag` is a `|`-separated list of `Key|value` requirements against the
    /// device's `Applicability\Tags`. A missing key fails its term; the literal
    /// `default` term passes.
    pub(crate) fn tag_applies(tag: &str, tags: &BTreeMap<String, String>) -> bool {
        let tag = tag.trim();
        if tag.is_empty() || tag.eq_ignore_ascii_case("default") {
            return true;
        }
        // Format is `Key|value` pairs separated by `,`. Split on commas first.
        for clause in tag.split(',') {
            let clause = clause.trim();
            if clause.is_empty() {
                continue;
            }
            let mut parts = clause.split('|');
            let key = parts.next().unwrap_or("").trim();
            let want = parts.next().unwrap_or("").trim();
            if key.is_empty() {
                continue;
            }
            match tags.get(key) {
                Some(have) if have.trim().eq_ignore_ascii_case(want) => {}
                _ => return false,
            }
        }
        true
    }

    /// Parse a semver bound like `<25.2.0.0`, `>=2.0`, `=1.2`, or bare `1.2.3`
    /// (treated as `>=`). Returns `(op, parts)`; `None` if not a valid bound.
    pub(crate) fn parse_range(range: &str) -> Option<(char, Vec<u64>)> {
        let range = range.trim();
        // Match two-character operators before their one-character prefixes so
        // `>=`/`<=` are not consumed as `>`/`<` plus a stray `=`.
        let (op, rest) = if let Some(rest) = range.strip_prefix("<=") {
            ('L', rest)
        } else if let Some(rest) = range.strip_prefix(">=") {
            ('G', rest)
        } else if let Some(rest) = range.strip_prefix('<') {
            ('l', rest)
        } else if let Some(rest) = range.strip_prefix('>') {
            ('g', rest)
        } else if let Some(rest) = range.strip_prefix('=') {
            ('=', rest)
        } else {
            ('g', range) // bare version = "at least"
        };
        let mut parts = Vec::new();
        for piece in rest.trim().split('.') {
            if piece.is_empty() {
                continue;
            }
            // Tolerate trailing non-digits ("25.2.0.0-beta") by trimming to the
            // leading numeric run; a piece with no digits fails the bound.
            let digits: String = piece.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return None;
            }
            parts.push(digits.parse().ok()?);
        }
        if parts.is_empty() {
            None
        } else {
            Some((op, parts))
        }
    }

    /// UDC emits some numeric fields as quoted decimal strings. Deserialize a
    /// `u64` that may arrive as `123`, `"123"`, or absent.
    pub(crate) fn deserialize_u64_flexible<'de, D>(
        deserializer: D,
    ) -> std::result::Result<Option<u64>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Num {
            U(u64),
            S(String),
        }
        match Option::<Num>::deserialize(deserializer)? {
            Some(Num::U(value)) => Ok(Some(value)),
            Some(Num::S(text)) => Ok(text.trim().parse().ok()),
            None => Ok(None),
        }
    }

    // ---------------------------------------------------------------------------
    // Registry read + the UpdateControl::check_updates implementation.
}

#[cfg(any(windows, test))]
pub(super) use catalog::*;

#[cfg(windows)]
mod imp {
    use super::*;
    use lctrl_core::CatalogPackage;
    use std::collections::BTreeMap;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, REG_VALUE_TYPE, RRF_RT_REG_SZ, RegGetValueW,
    };

    use crate::map_win_error;

    /// Read a REG_SZ/REG_EXPAND_SZ value as UTF-16 → String. `None` on absent.
    fn read_sz(key: &str, value: &str) -> Result<Option<String>> {
        let key_w = wide(key);
        let value_w = wide(value);
        let mut kind: REG_VALUE_TYPE = 0;
        let mut bytes: u32 = 0;
        // Size probe first.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key_w.as_ptr(),
                value_w.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if status == windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND
            || status == windows_sys::Win32::Foundation::ERROR_PATH_NOT_FOUND
        {
            return Ok(None);
        }
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("UDC registry value {value:?}"),
            ));
        }
        if bytes == 0 || bytes % 2 != 0 {
            return Ok(None);
        }
        let mut buffer = vec![0u16; (bytes / 2) as usize];
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key_w.as_ptr(),
                value_w.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut bytes,
            )
        };
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("UDC registry value {value:?}"),
            ));
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buffer.len());
        Ok(Some(String::from_utf16_lossy(&buffer[..len])))
    }

    /// Read every `REG_SZ` value under a key (the device `Applicability\Tags`).
    /// `RegEnumValueW` is required to enumerate; returns the name→value map.
    fn read_tag_map() -> BTreeMap<String, String> {
        use windows_sys::Win32::System::Registry::{
            HKEY, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
        };
        let mut tags = BTreeMap::new();
        let key_w = wide(UDC_TAGS_KEY);
        let mut handle: HKEY = std::ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, key_w.as_ptr(), 0, KEY_READ, &mut handle) };
        if status != 0 || handle.is_null() {
            return tags;
        }
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            let mut kind: u32 = 0;
            let mut data = [0u8; 2048];
            let mut data_len = data.len() as u32;
            let status = unsafe {
                RegEnumValueW(
                    handle,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    &mut kind,
                    data.as_mut_ptr(),
                    &mut data_len,
                )
            };
            if status != 0 {
                break;
            }
            index += 1;
            let name = String::from_utf16_lossy(&name[..name_len as usize]);
            if data_len >= 2 && data_len % 2 == 0 {
                let units = data_len as usize / 2;
                let text = unsafe {
                    String::from_utf16_lossy(std::slice::from_raw_parts(
                        data.as_ptr().cast::<u16>(),
                        units.saturating_sub(1).min(units),
                    ))
                };
                tags.insert(name, text);
            }
        }
        unsafe {
            RegCloseKey(handle);
        }
        tags
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    /// Whether a synced catalogue config exists and parses. Stops before
    /// applicability evaluation (`evaluate`), so it makes no claim about the
    /// packages this machine qualifies for.
    pub fn catalog_sync() -> Result<Option<CatalogSync>> {
        let Some(config) = read_config()? else {
            return Ok(None);
        };
        Ok(Some(CatalogSync {
            last_sync: read_sz(UDC_PACKAGES_KEY, "UpdateCheckDateTime")?
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            packages: config.packages.len(),
            stale: config.config_update_required.unwrap_or(0) == 1,
        }))
    }

    /// Read the synced UDC config + device facts and evaluate the catalog.
    /// Returns the report or an `Unsupported`/`Unavailable` error, never a
    /// fabricated package set.
    pub fn check_updates_impl(mtm: &str) -> Result<UpdateCheckReport> {
        let config = read_config()?.ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "UDC LastUpdateConfig is not synced in the registry".into(),
        })?;
        let org = read_sz(UDC_KEY, "registration-account")?
            .unwrap_or_default()
            .trim()
            .to_string();
        let tags = read_tag_map();
        let device = DeviceApplicability {
            org: &org,
            mtm,
            tags: &tags,
        };
        let packages = evaluate(&config, &device);
        let stale = config.config_update_required.unwrap_or(0) == 1 || packages.is_empty();
        Ok(UpdateCheckReport {
            // `UpdateCheckDateTime` is a sibling registry value, not a JSON
            // field — it is the catalog's sync provenance.
            last_sync: read_sz(UDC_PACKAGES_KEY, "UpdateCheckDateTime")?
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            stale,
            packages,
        })
    }

    /// The synced catalogue config, or `None` when the service has not written
    /// one. A config that is present but unparseable is an error, never an
    /// empty catalogue.
    fn read_config() -> Result<Option<UpdateConfig>> {
        let json = match read_sz(UDC_PACKAGES_KEY, "LastUpdateConfig")? {
            Some(json) if !json.trim().is_empty() => json,
            _ => return Ok(None),
        };
        let config: UpdateConfig =
            serde_json::from_str(&json).map_err(|error| LctrlError::ChannelUnavailable {
                channel: format!("UDC LastUpdateConfig is not parseable JSON: {error}"),
            })?;
        Ok(Some(config))
    }

    /// The catalogue entry for one package identifier, when the synced config
    /// still lists it. Used to bind an on-disk package directory to the
    /// package's name and type before its installer runs.
    pub fn package_binding(package_id: &str) -> Result<Option<CatalogPackage>> {
        let Some(config) = read_config()? else {
            return Ok(None);
        };
        Ok(config
            .packages
            .iter()
            .find(|package| {
                package
                    .package_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(package_id))
            })
            .map(to_catalog_package))
    }
}

#[cfg(windows)]
pub use imp::check_updates_impl;

#[cfg(windows)]
pub use imp::package_binding;

#[cfg(windows)]
pub use imp::catalog_sync;

/// Non-Windows build: the UDC registry channel does not exist, so no synced
/// catalogue can be proven — `Unsupported`, never an empty catalogue.
#[cfg(not(windows))]
pub fn catalog_sync() -> Result<Option<CatalogSync>> {
    Err(LctrlError::Unsupported {
        feature: "update.catalog.local".into(),
    })
}

/// Non-Windows build: the UDC registry channel does not exist, so the catalog
/// is honestly unavailable rather than fabricating an empty report.
#[cfg(not(windows))]
pub fn check_updates_impl(_mtm: &str) -> Result<UpdateCheckReport> {
    Err(LctrlError::ChannelUnavailable {
        channel: "Lenovo UDC catalog is only present on Windows".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn device<'a>(tags: &'a BTreeMap<String, String>) -> DeviceApplicability<'a> {
        DeviceApplicability {
            org: "LenovoAutoclaimed",
            mtm: "21VG",
            tags,
        }
    }

    fn package(filter_json: &str, orgs: &[&str], all: bool) -> RawPackage {
        RawPackage {
            package_name: Some("pkg".into()),
            package_version: Some("1.0".into()),
            package_type: None,
            package_update_type: None,
            package_size: None,
            package_manifest_url: None,
            package_url: None,
            package_file_hash: None,
            package_handler: None,
            applicability_filter: serde_json::from_str(filter_json).unwrap(),
            applicable_orgs: orgs.iter().map(|s| s.to_string()).collect(),
            apply_to_all_orgs: all,
        }
    }

    #[test]
    fn keeps_a_package_matching_device_mtm_and_org() {
        let tags = BTreeMap::new();
        let d = device(&tags);
        let pkg = package(
            r#"[{"mfr":"lenovo","mtm":"21VG","os":"windows","version":"<99.0"}]"#,
            &["LenovoAutoclaimed"],
            false,
        );
        assert!(package_applies(&pkg, &d));
    }

    #[test]
    fn drops_a_package_for_a_different_mtm() {
        let tags = BTreeMap::new();
        let d = device(&tags);
        let pkg = package(
            r#"[{"mfr":"lenovo","mtm":"OTHER","os":"windows"}]"#,
            &["LenovoAutoclaimed"],
            false,
        );
        assert!(!package_applies(&pkg, &d));
    }

    #[test]
    fn drops_a_package_for_a_non_member_org() {
        let tags = BTreeMap::new();
        let d = device(&tags);
        let pkg = package("[]", &["OtherOrg"], false);
        assert!(!package_applies(&pkg, &d));
    }

    #[test]
    fn apply_to_all_orgs_overrides_membership() {
        let tags = BTreeMap::new();
        let d = device(&tags);
        let pkg = package("[]", &["OtherOrg"], true);
        assert!(package_applies(&pkg, &d));
    }

    #[test]
    fn not_applicable_mtm_is_a_wildcard() {
        let tags = BTreeMap::new();
        let d = device(&tags);
        let pkg = package(
            r#"[{"mfr":"lenovo","mtm":"NotApplicableToOldUdc","os":"windows"}]"#,
            &["LenovoAutoclaimed"],
            false,
        );
        assert!(package_applies(&pkg, &d));
    }

    #[test]
    fn tag_terms_evaluate_against_device_tags() {
        let mut tags = BTreeMap::new();
        tags.insert("App.X.SystemSupport".to_string(), "true".to_string());
        let d = device(&tags);
        let pass = package(
            r#"[{"tag":"App.X.SystemSupport|true"}]"#,
            &["LenovoAutoclaimed"],
            false,
        );
        assert!(package_applies(&pass, &d));
        let fail = package(
            r#"[{"tag":"App.Y.SystemSupport|true"}]"#,
            &["LenovoAutoclaimed"],
            false,
        );
        assert!(!package_applies(&fail, &d));
    }

    #[test]
    fn parses_semver_range_bounds() {
        assert!(parse_range("<25.2.0.0").is_some());
        assert!(parse_range(">=2.0").is_some());
        assert!(parse_range("=1.2.3").is_some());
        assert!(parse_range("1.2.3").is_some());
        assert!(parse_range("not-a-version").is_none());
    }

    #[test]
    fn evaluates_a_representative_last_update_config() {
        let config: UpdateConfig = serde_json::from_str(
            r#"{
                "active_hours_end":"1800",
                "common_update_policy":["require_package_setup_validation"],
                "packages":[{
                    "package_name":"Lenovo AI Now",
                    "package_version":"25.1.0.0",
                    "package_type":"app",
                    "package_update_type":"optional",
                    "package_size":"2787227",
                    "package_url":"https://example/p",
                    "package_file_hash":"abc",
                    "applicability_filter":[{"mfr":"lenovo","mtm":"NotApplicableToOldUdc","os":"windows","version":"<25.2.0.0"}],
                    "applicableOrgs":["LenovoAutoclaimed"],
                    "applyToAllOrgs":false
                }]
            }"#,
        )
        .unwrap();
        let mut tags = BTreeMap::new();
        let d = DeviceApplicability {
            org: "LenovoAutoclaimed",
            mtm: "21VG",
            tags: &tags,
        };
        let offered = evaluate(&config, &d);
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].name, "Lenovo AI Now");
        tags.insert("x".into(), "y".into());
        let _ = &mut tags;
    }
}
