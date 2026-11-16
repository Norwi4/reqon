use anyhow::Result;
use std::sync::{
    Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub struct Jar {
    store: RwLock<cookie_store::CookieStore>,
    dirty: AtomicBool,
    saving: Mutex<()>,
}
#[derive(Clone, Default)]
pub struct Entry {
    pub domain: String,
    pub path: String,
    pub name: String,
    pub value: String,
    pub raw: String,
    pub expires: String,
    pub secure: bool,
    pub http_only: bool,
}
impl Jar {
    pub fn load(root: &std::path::Path) -> Result<Self> {
        let path = root.join("cookies.bin");
        let jar = Self::default();
        if path.exists() {
            let bytes = crate::secrets::unprotect(&std::fs::read(path)?)?;
            let store = cookie_store::serde::json::load(bytes.as_slice())
                .map_err(|e| anyhow::anyhow!("Cannot restore cookies: {e}"))?;
            *jar.store.write().unwrap() = store;
        }
        Ok(jar)
    }
    pub fn save(&self, root: &std::path::Path) -> Result<()> {
        let _serial = self.saving.lock().unwrap();
        let store = self.store.read().unwrap();
        if store.iter_unexpired().next().is_none() && !root.join("cookies.bin").exists() {
            return Ok(());
        }
        let values = store.iter_unexpired().cloned().collect::<Vec<_>>();
        drop(store);
        let bytes = serde_json::to_vec(&values)?;
        let bytes = crate::secrets::protect(&bytes)?;
        crate::storage::atomic_write(&root.join("cookies.bin"), bytes)
    }
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }
    pub fn entries(&self) -> Vec<Entry> {
        let store = self.store.read().unwrap();
        store
            .iter_unexpired()
            .map(|c| Entry {
                domain: match &c.domain {
                    cookie_store::CookieDomain::HostOnly(s)
                    | cookie_store::CookieDomain::Suffix(s) => s.clone(),
                    _ => String::new(),
                },
                path: c.path.to_string(),
                name: c.name().into(),
                value: c.value().into(),
                raw: c.to_string(),
                expires: match c.expires {
                    cookie_store::CookieExpiration::AtUtc(date) => date
                        .format(time::macros::format_description!(
                            "[year]-[month]-[day] [hour]:[minute]"
                        ))
                        .unwrap_or_default(),
                    _ => String::new(),
                },
                secure: c.secure().unwrap_or(false),
                http_only: c.http_only().unwrap_or(false),
            })
            .collect()
    }
    #[cfg(test)]
    pub fn add(&self, url: &url::Url, source: &str) -> Result<()> {
        self.replace(url, source, None)
    }
    pub fn replace(&self, url: &url::Url, source: &str, original: Option<&Entry>) -> Result<()> {
        let cookie = cookie_store::Cookie::parse(source.to_owned(), url)
            .map_err(|e| anyhow::anyhow!("Invalid cookie: {e}"))?;
        let domain = match &cookie.domain {
            cookie_store::CookieDomain::HostOnly(s) | cookie_store::CookieDomain::Suffix(s) => {
                s.clone()
            }
            _ => String::new(),
        };
        let same = original.is_some_and(|old| {
            old.domain == domain && old.path == cookie.path.to_string() && old.name == cookie.name()
        });
        let mut store = self.store.write().unwrap();
        store
            .insert(cookie, url)
            .map_err(|e| anyhow::anyhow!("Invalid cookie: {e}"))?;
        if !same {
            if let Some(old) = original {
                store.remove(&old.domain, &old.path, &old.name);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }
    pub fn remove(&self, entry: &Entry) {
        self.store
            .write()
            .unwrap()
            .remove(&entry.domain, &entry.path, &entry.name);
        self.dirty.store(true, Ordering::Relaxed);
    }
    pub fn clear(&self) {
        self.store.write().unwrap().clear();
        self.dirty.store(true, Ordering::Relaxed);
    }
}
impl reqwest::cookie::CookieStore for Jar {
    fn set_cookies(
        &self,
        headers: &mut dyn Iterator<Item = &reqwest::header::HeaderValue>,
        url: &url::Url,
    ) {
        let mut store = self.store.write().unwrap();
        let mut changed = false;
        for header in headers {
            if let Ok(value) = header.to_str() {
                changed |= store.parse(value, url).is_ok();
            }
        }
        if changed {
            self.dirty.store(true, Ordering::Relaxed);
        }
    }
    fn cookies(&self, url: &url::Url) -> Option<reqwest::header::HeaderValue> {
        let store = self.store.read().unwrap();
        let text = store
            .get_request_values(url)
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if text.is_empty() {
            None
        } else {
            text.parse().ok()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::cookie::CookieStore;
    #[test]
    fn cookies_obey_scope_and_survive_encrypted_local_restart() {
        let d = tempfile::tempdir().unwrap();
        let jar = Jar::default();
        let url = url::Url::parse("https://api.example.com/account/login").unwrap();
        jar.add(
            &url,
            "session=fixture-cookie; Path=/account; Secure; HttpOnly",
        )
        .unwrap();
        assert!(jar.cookies(&url).is_some());
        assert!(
            jar.cookies(&url::Url::parse("http://api.example.com/account").unwrap())
                .is_none()
        );
        assert!(
            jar.cookies(&url::Url::parse("https://api.example.com/other").unwrap())
                .is_none()
        );
        jar.save(d.path()).unwrap();
        let data = std::fs::read(d.path().join("cookies.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&data).contains("fixture-cookie"));
        let restored = Jar::load(d.path()).unwrap();
        assert_eq!(restored.cookies(&url).unwrap(), "session=fixture-cookie");
        restored
            .replace(
                &url,
                "renamed=new-value; Path=/account",
                Some(&restored.entries()[0]),
            )
            .unwrap();
        assert_eq!(restored.entries().len(), 1);
        assert_eq!(restored.cookies(&url).unwrap(), "renamed=new-value");
        restored.remove(&restored.entries()[0]);
        assert!(restored.cookies(&url).is_none());
    }
}

pub fn import_legacy(source: &std::path::Path, target: &std::path::Path) -> Result<()> {
    let mut store = cookie_store::CookieStore::default();
    load_legacy(
        &mut store,
        &std::fs::read_to_string(source.join("cookies.txt"))?,
    )?;
    let jar = Jar {
        store: RwLock::new(store),
        ..Default::default()
    };
    jar.save(target)
}
use anyhow::Context;
use cookie_store::{Cookie, CookieExpiration, CookieStore, RawCookie};
use url::Url;
fn load_legacy(store: &mut CookieStore, source: &str) -> Result<()> {
    for (index, line) in source.lines().enumerate() {
        let (http_only, line) = line
            .strip_prefix("#HttpOnly_")
            .map(|line| (true, line))
            .unwrap_or((false, line));
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.splitn(7, '\t').collect();
        anyhow::ensure!(
            fields.len() == 7,
            "Invalid legacy cookie record on line {}",
            index + 1
        );
        let [domain, subdomains, path, secure, expires, name, value] = fields.as_slice() else {
            unreachable!()
        };
        let secure = secure.eq_ignore_ascii_case("TRUE");
        let host = domain.trim_start_matches('.');
        let url = Url::parse(&format!(
            "{}://{host}{path}",
            if secure { "https" } else { "http" }
        ))
        .with_context(|| format!("Invalid legacy cookie domain on line {}", index + 1))?;
        let mut raw = RawCookie::build((name.to_string(), value.to_string()))
            .path(path.to_string())
            .secure(secure)
            .http_only(http_only);
        if subdomains.eq_ignore_ascii_case("TRUE") {
            raw = raw.domain(domain.to_string());
        }
        let mut cookie = Cookie::try_from_raw_cookie(&raw.build(), &url)
            .map_err(|_| anyhow::anyhow!("Invalid legacy cookie on line {}", index + 1))?
            .into_owned();
        if !expires.is_empty() && *expires != "0" {
            let timestamp = expires
                .parse::<i64>()
                .with_context(|| format!("Invalid cookie expiry on line {}", index + 1))?;
            cookie.expires =
                CookieExpiration::AtUtc(time::OffsetDateTime::from_unix_timestamp(timestamp)?);
            if cookie.is_expired() {
                continue;
            }
        }
        store
            .insert(cookie, &url)
            .map_err(|_| anyhow::anyhow!("Invalid legacy cookie on line {}", index + 1))?;
    }
    Ok(())
}
