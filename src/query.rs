use crate::model::Request;

/// Preserve the 0.19 wire query until its URL is edited in the native editor.
pub fn legacy_url(
    r: &Request,
    vars: &std::collections::BTreeMap<String, String>,
) -> Option<anyhow::Result<url::Url>> {
    if r.portable["__legacy_query_url"].as_str() != Some(r.url.as_str()) {
        return None;
    }
    Some((|| {
        let resolve = |s: &str| crate::model::resolve(s, vars);
        let base = resolve(&r.url);
        anyhow::ensure!(
            !base.contains("{{"),
            "The URL contains unresolved variables. Select an environment or define them in Vars."
        );
        let mut url = url::Url::parse(&base)?;
        let rows: Vec<_> = r
            .params
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .map(|p| (&p.key, &p.value))
            .collect();
        let encode = r.portable["http_encode_url"] != false;
        let pair = |k: &str, v: &str| {
            if encode {
                const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
                    .remove(b'-')
                    .remove(b'_')
                    .remove(b'.')
                    .remove(b'~');
                let escape = |s: &str| {
                    percent_encoding::utf8_percent_encode(s, SET)
                        .to_string()
                        .replace("%20", "+")
                };
                format!("{}={}", escape(k), escape(v))
            } else {
                format!("{k}={v}")
            }
        };
        let mut query = if r.portable["imported_query"].is_string()
            && serde_json::json!(&rows) == r.portable["imported_query_params"]
        {
            resolve(r.portable["imported_query"].as_str().unwrap())
        } else {
            rows.iter()
                .map(|(k, v)| pair(&resolve(k), &resolve(v)))
                .collect::<Vec<_>>()
                .join("&")
        };
        if r.auth.kind == "apikey" && r.auth.in_query {
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(&pair(&resolve(&r.auth.key), &resolve(&r.auth.value)));
        }
        if !query.is_empty() {
            if let Some(existing) = url.query().filter(|q| !q.is_empty()) {
                query = format!("{existing}&{query}");
            }
            url.set_query(Some(&query));
        }
        url.set_fragment(None);
        Ok(url)
    })())
}

// The editor keeps raw query values. Encoding happens at the wire boundary.
pub fn update_rows(request: &mut Request) {
    let Some((_, query)) = request.url.split_once('?') else {
        request.params.retain(|p| !p.enabled);
        return;
    };
    let query = if request.encode_url {
        query
    } else {
        query.split('#').next().unwrap_or(query)
    };
    let mut old = std::mem::take(&mut request.params);
    let mut rows = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key.is_empty() {
            continue;
        }
        let index = old.iter().position(|p| p.enabled && p.key == key);
        let mut row = index.map(|i| old.remove(i)).unwrap_or_default();
        row.key = key.into();
        row.value = value.into();
        row.enabled = true;
        rows.push(row);
    }
    rows.extend(old.into_iter().filter(|p| !p.enabled && !p.key.is_empty()));
    request.params = rows;
}
pub fn update_url(request: &mut Request) {
    let (url, fragment) = if request.encode_url {
        (request.url.as_str(), String::new())
    } else {
        request
            .url
            .split_once('#')
            .map(|(a, b)| (a, format!("#{b}")))
            .unwrap_or((&request.url, String::new()))
    };
    let base = url.split('?').next().unwrap_or(url);
    let query = request
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| format!("{}={}", p.key, p.value))
        .collect::<Vec<_>>()
        .join("&");
    request.url = if query.is_empty() {
        format!("{base}{fragment}")
    } else {
        format!("{base}?{query}{fragment}")
    };
}
pub fn component(value: &str) -> String {
    const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    percent_encoding::utf8_percent_encode(value, SET).to_string()
}
pub fn encode_url(url: &str) -> String {
    let (base, query) = url
        .split_once('?')
        .map(|(a, b)| (a, Some(b)))
        .unwrap_or((url, None));
    let authority = base.find("://").map(|i| i + 3).unwrap_or(0);
    let path_start = base[authority..]
        .find(['/', '#'])
        .map(|i| authority + i)
        .unwrap_or(base.len());
    let path = base[path_start..]
        .split('/')
        .map(|segment| {
            let decoded = percent_encoding::percent_decode_str(segment)
                .decode_utf8()
                .unwrap_or_else(|_| segment.into());
            component(&decoded)
        })
        .collect::<Vec<_>>()
        .join("/");
    let mut out = format!("{}{path}", &base[..path_start]);
    if let Some(query) = query {
        let query = query
            .split('&')
            .filter_map(|pair| {
                let (name, value) = pair
                    .split_once('=')
                    .map(|(a, b)| (a, Some(b)))
                    .unwrap_or((pair, None));
                if name.trim().is_empty() {
                    return None;
                }
                Some(match value {
                    Some(value) => format!("{}={}", component(name), component(value)),
                    None => component(name),
                })
            })
            .collect::<Vec<_>>()
            .join("&");
        if !query.is_empty() {
            out.push('?');
            out.push_str(&query);
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Pair;
    #[test]
    fn migrated_query_preserves_019_encoding_duplicates_and_raw_imports() {
        let mut r = Request {
            url: "https://example.test/?base=one".into(),
            params: vec![Pair::new("q", "a&b"), Pair::new("q", "{{token}}")],
            ..Default::default()
        };
        r.portable = serde_json::json!({"__legacy_query_url":r.url});
        let vars = std::collections::BTreeMap::from([("token".into(), "x+y".into())]);
        assert_eq!(
            crate::engine::request_url(&r, &vars).unwrap().query(),
            Some("base=one&q=a%26b&q=x%2By")
        );
        r.portable["imported_query_params"] = serde_json::json!([["q", "a&b"], ["q", "{{token}}"]]);
        r.portable["imported_query"] = serde_json::json!("q=a%26b&q={{token}}&flag");
        assert_eq!(
            crate::engine::request_url(&r, &vars).unwrap().query(),
            Some("base=one&q=a%26b&q=x+y&flag")
        );
        r.url = "https://example.test/new".into();
        assert!(legacy_url(&r, &vars).is_none());
    }
    #[test]
    fn roundtrip_duplicate_keys_disabled_rows_and_variables() {
        let mut r = Request {
            encode_url: false,
            url: "{{base}}/items?a=one&a=two#part".into(),
            params: vec![Pair {
                enabled: false,
                ..Pair::new("off", "kept")
            }],
            ..Default::default()
        };
        update_rows(&mut r);
        assert_eq!(r.params.len(), 3);
        assert!(!r.params[2].enabled);
        r.params[0].value = "{{token}} %20 value".into();
        update_url(&mut r);
        assert_eq!(r.url, "{{base}}/items?a={{token}} %20 value&a=two#part");
        update_rows(&mut r);
        assert_eq!(r.params[0].value, "{{token}} %20 value");
    }
    #[test]
    fn encoding_matches_bruno_path_and_query_contract() {
        assert_eq!(
            encode_url("https://example.com/a%20b/a b?q=%20&token=a#b&flag&x=a=b&empty="),
            "https://example.com/a%20b/a%20b?q=%2520&token=a%23b&flag&x=a%3Db&empty="
        );
        assert_eq!(
            encode_url("http://[::1]:8080/a#b?q=Привет мир"),
            "http://[::1]:8080/a%23b?q=%D0%9F%D1%80%D0%B8%D0%B2%D0%B5%D1%82%20%D0%BC%D0%B8%D1%80"
        );
    }
    #[test]
    fn query_hash_is_encoded_as_data_after_editor_sync() {
        let mut r = Request {
            url: "https://example.com?q=a#b&next=c".into(),
            encode_url: true,
            ..Default::default()
        };
        update_rows(&mut r);
        assert_eq!(r.params[0].value, "a#b");
        assert_eq!(
            crate::engine::request_url(&r, &Default::default())
                .unwrap()
                .as_str(),
            "https://example.com/?q=a%23b&next=c"
        );
        r.params[0].value = "x#y".into();
        update_url(&mut r);
        assert_eq!(r.url, "https://example.com?q=x#y&next=c");
    }
}
