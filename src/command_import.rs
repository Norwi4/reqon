//! Parse pasted commands as data: no shell, filesystem or network access.
fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn query_pair(key: &str, value: &str) -> String {
    const ESCAPE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    format!(
        "{}={}",
        percent_encoding::utf8_percent_encode(key, ESCAPE),
        percent_encoding::utf8_percent_encode(value, ESCAPE)
    )
}
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Imported {
    pub data: Value,
    pub notes: Vec<String>,
}
fn escaped(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'a' => '\x07',
        'b' => '\x08',
        'f' => '\x0c',
        'v' => '\x0b',
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        _ => return None,
    })
}
pub fn split_command(source: &str, shell: &str) -> Result<Vec<String>> {
    let source = source.trim().replace("\r\n", "\n");
    ensure!(!source.is_empty(), "Paste a cURL or gRPCurl command.");
    ensure!(
        source.len() <= 2_000_000,
        "The command is too large (maximum 2 MB)."
    );
    let shell = if shell == "auto" {
        if source
            .split_inclusive('\n')
            .any(|l| l.ends_with('\n') && l.trim_end().ends_with('^'))
        {
            "cmd"
        } else if source
            .split_inclusive('\n')
            .any(|l| l.ends_with('\n') && l.trim_end().ends_with('`'))
        {
            "powershell"
        } else {
            "bash"
        }
    } else {
        shell
    };
    ensure!(
        matches!(shell, "bash" | "cmd" | "powershell"),
        "Unknown shell syntax"
    );
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    let mut quote = '\0';
    let mut started = false;
    let mut buffer = String::new();
    let mut tokens = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if quote == 'a' {
            if c == '\'' {
                quote = '\0';
                i += 1;
                continue;
            }
            if c == '\\' {
                if let Some(n) = next {
                    let spec = match n {
                        'x' => Some((16, 1, 2, 2)),
                        'u' => Some((16, 4, 4, 2)),
                        'U' => Some((16, 8, 8, 2)),
                        '0'..='7' => Some((8, 1, 3, 1)),
                        _ => None,
                    };
                    if let Some((radix, min, max, offset)) = spec {
                        let digits: String = chars
                            .iter()
                            .skip(i + offset)
                            .take(max)
                            .take_while(|c| c.is_digit(radix))
                            .collect();
                        if digits.len() >= min {
                            let number = u32::from_str_radix(&digits, radix)?;
                            buffer.push(
                                char::from_u32(number)
                                    .context("Invalid character escape in the command")?,
                            );
                            i += offset + digits.len();
                            continue;
                        }
                    }
                    if let Some(value) = escaped(n) {
                        buffer.push(value);
                    } else {
                        buffer.push('\\');
                        buffer.push(n);
                    }
                    i += 2;
                    continue;
                }
            }
            buffer.push(c);
            i += 1;
            continue;
        }
        if quote == '\'' {
            if c == '\'' {
                if shell == "powershell" && next == Some('\'') {
                    buffer.push('\'');
                    i += 2;
                    continue;
                }
                quote = '\0';
            } else {
                buffer.push(c);
            }
            i += 1;
            continue;
        }
        if shell == "cmd" && chars.get(i..i + 4) == Some(&['^', '\\', '^', '"']) {
            buffer.push('"');
            started = true;
            i += 4;
            continue;
        }
        let escape = if shell == "powershell" {
            '`'
        } else if shell == "cmd" {
            '^'
        } else {
            '\\'
        };
        if c == escape {
            let n = next.context("The command ends with an incomplete escape.")?;
            if n == '\n' {
                i += 2;
                continue;
            }
            if shell == "bash" && quote == '"' && !['\\', '"', '$', '`'].contains(&n) {
                buffer.push(c);
                i += 1;
                continue;
            }
            buffer.push(if shell == "powershell" && quote == '"' {
                escaped(n).unwrap_or(n)
            } else {
                n
            });
            started = true;
            i += 2;
            continue;
        }
        if shell == "cmd" && quote == '"' && c == '\\' && next == Some('"') {
            buffer.push('"');
            i += 2;
            continue;
        }
        if c == '"' {
            quote = if quote == '"' { '\0' } else { '"' };
            started = true;
            i += 1;
            continue;
        }
        ensure!(
            c != '`' && !(c == '$' && next == Some('(')),
            "Shell substitutions are not supported. Paste the actual values instead."
        );
        if quote == '\0' {
            if shell == "bash" && c == '$' && next == Some('\'') {
                quote = 'a';
                started = true;
                i += 2;
                continue;
            }
            if c == '\'' && shell != "cmd" {
                quote = '\'';
                started = true;
                i += 1;
                continue;
            }
            if c.is_whitespace() {
                if started {
                    tokens.push(std::mem::take(&mut buffer));
                    started = false;
                }
                i += 1;
                continue;
            }
            ensure!(
                !";|&<>".contains(c),
                "Paste one command without pipes or redirection; quote URLs and values containing shell operators."
            );
            if c == '#' && !started {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
        }
        buffer.push(c);
        started = true;
        i += 1;
    }
    ensure!(quote == '\0', "Close the quotation marks in the command.");
    if started {
        tokens.push(buffer);
    }
    ensure!(
        tokens.iter().all(|t| !t.contains('\0')),
        "NUL bytes cannot be imported into a text request."
    );
    Ok(tokens)
}

const CURL_VALUES: &[(&str, &str)] = &[
    ("--proxy", "proxy"),
    ("-x", "proxy"),
    ("-X", "method"),
    ("--request", "method"),
    ("--url", "url"),
    ("-H", "header"),
    ("--header", "header"),
    ("-d", "data"),
    ("--data", "data"),
    ("--data-ascii", "data"),
    ("--data-raw", "raw"),
    ("--data-binary", "binary"),
    ("--data-urlencode", "encoded"),
    ("--json", "json"),
    ("-u", "user"),
    ("--user", "user"),
    ("--oauth2-bearer", "bearer"),
    ("-b", "cookie"),
    ("--cookie", "cookie"),
    ("-A", "agent"),
    ("--user-agent", "agent"),
    ("-e", "referer"),
    ("--referer", "referer"),
    ("-m", "timeout"),
    ("--max-time", "timeout"),
    ("--cacert", "ca"),
    ("--cert", "cert"),
    ("-E", "cert"),
    ("--key", "key"),
    ("--url-query", "query"),
    ("-F", "multipart"),
    ("--form", "multipart"),
    ("--form-string", "form_string"),
    ("--max-redirs", "max_redirects"),
    ("-o", "output"),
    ("--output", "output"),
    ("-w", "output"),
    ("--write-out", "output"),
];
const CURL_FLAGS: &[(&str, &str)] = &[
    ("--digest", "digest"),
    ("--ntlm", "ntlm"),
    ("-G", "get"),
    ("--get", "get"),
    ("-I", "head"),
    ("--head", "head"),
    ("-L", "redirect"),
    ("--location", "redirect"),
    ("--no-location", "no_redirect"),
    ("-k", "insecure"),
    ("--insecure", "insecure"),
    ("--no-insecure", "secure"),
    ("--compressed", "compressed"),
    ("--basic", "ignore"),
    ("-g", "ignore"),
    ("--globoff", "ignore"),
    ("--http1.1", "http1"),
    ("--http2", "http_auto"),
    ("--http2-prior-knowledge", "http2"),
    ("-s", "ignore"),
    ("--silent", "ignore"),
    ("-S", "ignore"),
    ("--show-error", "ignore"),
    ("-v", "ignore"),
    ("--verbose", "ignore"),
    ("-i", "ignore"),
    ("--include", "ignore"),
    ("--show-headers", "ignore"),
    ("-f", "ignore"),
    ("--fail", "ignore"),
    ("--location-trusted", "trusted"),
    ("--post301", "original_method"),
    ("--post302", "original_method"),
    ("--post303", "original_method"),
    ("--fail-with-body", "ignore"),
    ("--no-progress-meter", "ignore"),
    ("-N", "ignore"),
    ("--no-buffer", "ignore"),
];
fn alias<'a>(list: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    list.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}
fn options(
    tokens: &[String],
    values: &[(&str, &str)],
    flags: &[(&str, &str)],
    short: bool,
) -> Result<(Vec<(String, String)>, Vec<String>)> {
    let mut result = Vec::new();
    let mut args = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        i += 1;
        if token == "--" {
            args.extend_from_slice(&tokens[i..]);
            break;
        }
        if !token.starts_with('-') || token == "-" {
            args.push(token.clone());
            continue;
        }
        let (key, inline) = token
            .split_once('=')
            .map(|(k, v)| (k, Some(v)))
            .unwrap_or((token, None));
        if let Some(name) = alias(values, key) {
            let value = if let Some(v) = inline {
                v.to_owned()
            } else {
                let v = tokens
                    .get(i)
                    .with_context(|| format!("{key} needs a value."))?;
                i += 1;
                v.clone()
            };
            result.push((name.into(), value));
            continue;
        }
        if let Some(name) = alias(flags, key) {
            ensure!(inline.is_none(), "{key} does not take a value.");
            result.push((name.into(), String::new()));
            continue;
        }
        if short && !token.starts_with("--") {
            let mut chars = token[1..].chars();
            while let Some(c) = chars.next() {
                let key = format!("-{c}");
                if let Some(name) = alias(values, &key) {
                    let rest = chars.as_str();
                    let value = if rest.is_empty() {
                        let v = tokens
                            .get(i)
                            .with_context(|| format!("{key} needs a value."))?;
                        i += 1;
                        v.clone()
                    } else {
                        rest.into()
                    };
                    result.push((name.into(), value));
                    break;
                }
                let name = alias(flags, &key).with_context(|| {
                    format!(
                        "Unsupported option: {key}. Remove it or configure this request manually."
                    )
                })?;
                result.push((name.into(), String::new()));
            }
            continue;
        }
        bail!("Unsupported option: {key}. Remove it or configure this request manually.");
    }
    Ok((result, args))
}
fn inline(value: &str, option: &str) -> Result<()> {
    ensure!(
        !value.starts_with('@'),
        "{option} references a file or stdin. Paste its contents inline instead."
    );
    Ok(())
}
fn number(value: &str, option: &str, max: f64) -> Result<f64> {
    let n: f64 = value
        .parse()
        .with_context(|| format!("{option} needs a non-negative number."))?;
    ensure!(
        n.is_finite() && n >= 0. && n <= max,
        "{option} must be between 0 and {max}."
    );
    Ok(n)
}
fn header(value: &str) -> Result<Value> {
    inline(value, "Header")?;
    let (k, v) = if let Some((k, v)) = value.split_once(':') {
        (k, v)
    } else if let Some(k) = value.strip_suffix(';') {
        (k, "")
    } else {
        bail!("Headers must use Name: value.")
    };
    ensure!(
        !k.trim().is_empty() && !value.contains(['\r', '\n']),
        "Headers must use Name: value."
    );
    Ok(json!([k.trim(), v.trim_start(), ""]))
}
fn authorize(data: &mut Value, rows: &mut Vec<Value>, grpc: bool) {
    let matches: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r[0].as_str()
                .unwrap_or_default()
                .eq_ignore_ascii_case("authorization")
        })
        .map(|(i, _)| i)
        .collect();
    if let [index] = matches.as_slice() {
        let value = rows[*index][1].as_str().unwrap_or_default();
        let (scheme, value) = value.split_once(' ').unwrap_or((value, ""));
        if scheme.eq_ignore_ascii_case("bearer") {
            data["auth_type"] = json!("Bearer Token");
            data["auth_value"] = json!(value);
            rows.remove(*index);
        } else if scheme.eq_ignore_ascii_case("basic") && !grpc {
            if let Ok(bytes) = STANDARD.decode(value) {
                if let Ok(decoded) = String::from_utf8(bytes) {
                    if decoded.contains(':') {
                        data["auth_type"] = json!("Basic Auth");
                        data["auth_value"] = json!(decoded);
                        rows.remove(*index);
                    }
                }
            }
        }
    }
}
const URL_ESCAPE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
fn encoded(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, URL_ESCAPE).to_string()
}
fn encoded_data(value: &str) -> Result<String> {
    if let Some(value) = value.strip_prefix('=') {
        Ok(encoded(value))
    } else if let Some((k, v)) = value.split_once('=') {
        Ok(format!("{k}={}", encoded(v)))
    } else {
        ensure!(
            !value.contains('@'),
            "--data-urlencode references a file. Paste its contents inline instead."
        );
        Ok(encoded(value))
    }
}
fn pairs(value: &str) -> Vec<Value> {
    url::form_urlencoded::parse(value.as_bytes())
        .map(|(k, v)| json!([k, v, ""]))
        .collect()
}
fn default_header(rows: &mut Vec<Value>, key: &str, value: &str) {
    if !rows
        .iter()
        .any(|r| r[0].as_str().unwrap_or_default().eq_ignore_ascii_case(key))
    {
        rows.push(json!([key, value, ""]));
    }
}

fn curl(tokens: &[String]) -> Result<Imported> {
    let (opts, mut urls) = options(tokens, CURL_VALUES, CURL_FLAGS, true)?;
    let mut data = json!({"kind":"http","name":"Imported request","method":"GET","url":"","params":[],"headers":[],"auth_type":"No Auth","auth_value":"","body_type":"none","body":"","form":[],"http_verify":true,"http_follow_redirects":false,"http_timeout":0.0,"http_ca_file":"","http_cert_file":"","http_key_file":""});
    let mut notes = Vec::new();
    let mut headers = Vec::new();
    let mut suppressed_headers = Vec::new();
    let mut payload = String::new();
    let mut query = Vec::new();
    let mut cookies = Vec::new();
    let mut multipart = Vec::new();
    let mut binary = None;
    let mut has_inline = false;
    let (mut explicit, mut get, mut head, mut json_body, mut compressed, mut has_body) =
        (None, false, false, false, false, false);
    for (key, value) in opts {
        match key.as_str() {
            "url" => urls.push(value),
            "method" => explicit = Some(value),
            "proxy" => data["http_proxy"] = json!(value),
            "digest" | "ntlm" => data["http_auth_scheme"] = json!(key),
            "header" => {
                let mut row = header(&value)?;
                let name = row[0].as_str().unwrap();
                if value.contains(':')
                    && row[1] == ""
                    && crate::generated_headers::configurable(name)
                {
                    suppressed_headers.push(name.to_ascii_lowercase());
                    row.as_array_mut().unwrap().push(json!(true));
                }
                headers.push(row);
            }
            "data" | "raw" | "binary" | "encoded" | "json" => {
                if key == "binary" && value.starts_with('@') {
                    ensure!(
                        value != "@-" && binary.is_none() && !has_inline,
                        "Import one binary file, without stdin or inline data."
                    );
                    binary = Some(value[1..].to_string());
                    has_body = true;
                    continue;
                }
                ensure!(
                    binary.is_none(),
                    "A binary file cannot be combined with inline data."
                );
                if key != "raw" {
                    inline(&value, &key)?;
                }
                let value = if key == "encoded" {
                    encoded_data(&value)?
                } else {
                    value
                };
                if has_inline && key != "json" {
                    payload.push('&');
                }
                payload.push_str(&value);
                has_inline = true;
                has_body = true;
                json_body |= key == "json";
            }
            "multipart" | "form_string" => {
                let (name, content) = value
                    .split_once('=')
                    .context("Form fields must use name=value.")?;
                ensure!(!name.is_empty(), "Form fields need a name.");
                let mut content = content.to_string();
                let mut kind = "Text";
                if key == "multipart" && content.starts_with('@') {
                    content.remove(0);
                    kind = "File";
                    if content.starts_with('"') && content.ends_with('"') && content.len() >= 2 {
                        content = content[1..content.len() - 1]
                            .replace("\\\"", "\"")
                            .replace("\\\\", "\\");
                    } else {
                        ensure!(
                            !content.contains([',', ';']),
                            "Use one file per form field without curl file modifiers."
                        );
                    }
                    ensure!(
                        !content.is_empty() && content != "-",
                        "Select a local file instead of stdin."
                    );
                } else if key == "multipart" {
                    ensure!(
                        !content.starts_with('<') && !content.contains(";type="),
                        "Paste a plain form value or use --form-string."
                    );
                }
                multipart.push(json!([name, content, "", true, kind]));
                has_body = true;
            }
            "query" => query.push(if let Some(v) = value.strip_prefix('+') {
                v.into()
            } else {
                encoded_data(&value)?
            }),
            "get" => get = true,
            "head" => head = true,
            "user" => {
                ensure!(value.contains(':'), "--user needs username:password.");
                data["auth_type"] = json!("Basic Auth");
                data["auth_value"] = json!(value);
            }
            "bearer" => {
                data["auth_type"] = json!("Bearer Token");
                data["auth_value"] = json!(value);
            }
            "cookie" => {
                ensure!(
                    value.contains('='),
                    "--cookie must contain inline cookies; cookie files are not imported."
                );
                cookies.push(value);
            }
            "agent" | "referer" => headers.push(json!([
                if key == "agent" {
                    "User-Agent"
                } else {
                    "Referer"
                },
                value,
                ""
            ])),
            "insecure" | "secure" => data["http_verify"] = json!(key == "secure"),
            "redirect" | "no_redirect" => data["http_follow_redirects"] = json!(key == "redirect"),
            "timeout" => data["http_timeout"] = json!(number(&value, "--max-time", 2147483.647)?),
            "max_redirects" => {
                data["http_max_redirects"] = json!(number(&value, "--max-redirs", 100.)? as u64)
            }
            "trusted" => {
                data["http_follow_redirects"] = json!(true);
                data["http_follow_auth"] = json!(true);
            }
            "original_method" => data["http_follow_original_method"] = json!(true),
            "ca" | "cert" | "key" => data[format!("http_{key}_file")] = json!(value),
            "compressed" => compressed = true,
            "http1" | "http_auto" | "http2" => {
                data["http_version"] = json!(match key.as_str() {
                    "http1" => "HTTP/1.x",
                    "http_auto" => "Auto",
                    _ => "HTTP/2",
                })
            }
            "output" => {
                let note =
                    "Output formatting/file options are omitted; the response appears in Reqon."
                        .to_string();
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
            _ => {}
        }
    }
    ensure!(urls.len() == 1, "Import one request URL at a time.");
    let raw = if urls[0].contains("://") {
        urls.remove(0)
    } else {
        format!("http://{}", urls.remove(0))
    };
    let parsed = url::Url::parse(&raw).context("The request URL is invalid.")?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some(),
        "cURL import supports HTTP and HTTPS URLs."
    );
    let (_, rest) = raw.split_once("://").unwrap();
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let path = rest[end..].split(['?', '#']).next().unwrap_or("");
    if authority.contains('@') && data["auth_type"] == "No Auth" {
        data["auth_type"] = json!("Basic Auth");
        data["auth_value"] = json!(format!(
            "{}:{}",
            percent_encoding::percent_decode_str(parsed.username()).decode_utf8_lossy(),
            percent_encoding::percent_decode_str(parsed.password().unwrap_or(""))
                .decode_utf8_lossy()
        ));
    }
    if !cookies.is_empty() {
        headers.push(json!(["Cookie", cookies.join("; "), ""]));
    }
    let mut seen = std::collections::HashSet::new();
    for row in &headers {
        let name = row[0].as_str().unwrap_or_default();
        ensure!(
            seen.insert(name.to_lowercase()),
            "Repeated HTTP header {name} cannot be imported separately. Combine its values first."
        );
    }
    authorize(&mut data, &mut headers, false);
    ensure!(
        multipart.is_empty() || (!has_inline && binary.is_none()),
        "Multipart fields cannot be combined with another body type."
    );
    ensure!(
        !get || (multipart.is_empty() && binary.is_none()),
        "--get cannot be combined with file or multipart bodies."
    );
    if get && has_body {
        query.push(payload.clone());
        has_body = false;
    }
    ensure!(
        !head || !has_body,
        "A HEAD request cannot be imported with a request body."
    );
    let method = explicit.unwrap_or_else(|| {
        if head {
            "HEAD"
        } else if get {
            "GET"
        } else if has_body {
            "POST"
        } else {
            "GET"
        }
        .into()
    });
    ensure!(
        !method.is_empty()
            && method
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+.^_`|~-".contains(c)),
        "The HTTP method is invalid."
    );
    data["method"] = json!(method);
    if let Some(raw_query) = raw
        .split('#')
        .next()
        .unwrap_or("")
        .split_once('?')
        .map(|(_, q)| q)
    {
        if !raw_query.is_empty() {
            query.insert(0, raw_query.to_owned());
        }
    }
    let raw_query = query.join("&");
    let params = pairs(&raw_query);
    data["imported_query_params"] = json!(
        params
            .iter()
            .filter(|r| r[0] != "")
            .map(|r| json!([r[0], r[1]]))
            .collect::<Vec<_>>()
    );
    data["params"] = json!(params);
    data["imported_query"] = json!(raw_query);
    data["url"] = json!(format!(
        "{}://{}{}",
        parsed.scheme(),
        authority.rsplit('@').next().unwrap(),
        path
    ));
    if !multipart.is_empty() {
        data["body_type"] = json!("form-data");
        data["multipart_rows"] = json!(multipart);
    } else if let Some(file) = binary {
        data["body_type"] = json!("binary");
        data["body_file"] = json!(file);
    } else if has_body {
        if json_body {
            default_header(&mut headers, "Content-Type", "application/json");
            default_header(&mut headers, "Accept", "application/json");
        } else {
            default_header(
                &mut headers,
                "Content-Type",
                "application/x-www-form-urlencoded",
            );
        }
        let content_type = headers
            .iter()
            .find(|r| {
                r[0].as_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case("content-type")
            })
            .and_then(|r| r[1].as_str())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let form = pairs(&payload);
        let encoded = form
            .iter()
            .map(|r| query_pair(r[0].as_str().unwrap(), r[1].as_str().unwrap()))
            .collect::<Vec<_>>()
            .join("&");
        if content_type == "application/x-www-form-urlencoded"
            && !payload.is_empty()
            && encoded == payload
            && form.iter().all(|r| r[0] != "")
        {
            data["body_type"] = json!("x-www-form-urlencoded");
            data["form"] = json!(form);
        } else {
            data["body_type"] = json!("raw");
            data["body"] = json!(payload);
        }
    }
    if compressed {
        default_header(&mut headers, "Accept-Encoding", "gzip, deflate");
    }
    headers.retain(|row| row.get(3) != Some(&json!(true)));
    for name in suppressed_headers {
        data["generated_headers"][name] = json!({"disabled":true});
    }
    data["headers"] = json!(headers);
    data["name"] = json!(format!(
        "{method} {}",
        if path.is_empty() { "/" } else { path }
    ));
    Ok(Imported { data, notes })
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}
fn grpcurl(tokens: &[String]) -> Result<Imported> {
    let values = &[
        ("-H", "header"),
        ("-rpc-header", "header"),
        ("-d", "message"),
        ("-cacert", "tls_ca_file"),
        ("-cert", "tls_cert_file"),
        ("-key", "tls_key_file"),
        ("-authority", "tls_server_name"),
        ("-protoset", "descriptor_file"),
        ("-proto", "proto"),
        ("-import-path", "include"),
        ("-max-time", "timeout"),
        ("-max-msg-sz", "size"),
        ("-format", "format"),
    ];
    let flags = &[
        ("-plaintext", "plaintext"),
        ("-insecure", "insecure"),
        ("-emit-defaults", "defaults"),
        ("-v", "ignore"),
        ("-vv", "ignore"),
    ];
    let (opts, args) = options(tokens, values, flags, false)?;
    ensure!(
        args.len() == 2 && !matches!(args[1].as_str(), "list" | "describe"),
        "Paste a gRPCurl invocation with a server and Service/Method."
    );
    let method = args[1].trim_start_matches('/');
    let (service, rpc) = method
        .rsplit_once('/')
        .or_else(|| method.rsplit_once('.'))
        .context("Use package.Service/Method.")?;
    ensure!(
        service.split('.').all(identifier) && identifier(rpc),
        "Use package.Service/Method."
    );
    let mut server = args[0].trim().to_owned();
    ensure!(
        !server.contains("{{"),
        "The server URL contains an unresolved variable."
    );
    if server.contains("://") {
        let url = url::Url::parse(&server)?;
        ensure!(
            matches!(url.scheme(), "grpc" | "grpcs" | "http" | "https")
                && matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty(),
            "The gRPC server URL must contain only a host and optional port."
        );
        server = server
            .split_once("://")
            .unwrap()
            .1
            .trim_end_matches('/')
            .into();
    } else {
        server = server.trim_end_matches('/').into();
    }
    ensure!(
        !server.is_empty()
            && !server
                .chars()
                .any(|c| c.is_whitespace() || "/?#@".contains(c)),
        "Enter a valid gRPC host:port without a path."
    );
    let parsed = url::Url::parse(&format!("http://{server}"))?;
    ensure!(parsed.host_str().is_some(), "Enter a valid gRPC host:port.");
    let mut data = json!({"kind":"grpc","name":rpc,"server":server,"service":service,"rpc_method":rpc,"message":"{}","metadata":[],"tls":true,"tls_verify":true,"auth_type":"No Auth","auth_value":"","descriptor_file":"","grpc_emit_defaults":false,"grpc_timeout_ms":0,"grpc_max_response_mb":4});
    let mut notes = Vec::new();
    let mut metadata = Vec::new();
    let mut protos = Vec::new();
    let mut includes = Vec::new();
    for (key, value) in opts {
        match key.as_str() {
            "header" => metadata.push(header(&value)?),
            "message" => {
                ensure!(
                    value != "@",
                    "gRPCurl -d @ reads stdin. Paste Message JSON inline instead."
                );
                data["message"] = json!(value);
            }
            "plaintext" => data["tls"] = json!(false),
            "proto" => protos.push(value),
            "include" => includes.push(value),
            "insecure" => data["tls_verify"] = json!(false),
            "defaults" => data["grpc_emit_defaults"] = json!(true),
            "timeout" => {
                data["grpc_timeout_ms"] =
                    json!((number(&value, "-max-time", 2147483.647)? * 1000.).round() as u64)
            }
            "size" => {
                let size = number(&value, "-max-msg-sz", 2147483647.)?;
                ensure!(
                    size.fract() == 0.,
                    "-max-msg-sz needs a whole number of bytes."
                );
                let size = size as u64;
                data["grpc_max_response_mb"] = json!(if size == 2147483647 {
                    0
                } else if size == 0 {
                    4
                } else {
                    size.div_ceil(1048576).clamp(1, 2047)
                });
                if size != 2147483647 && size % 1048576 != 0 {
                    notes.push(
                        "Maximum response size is rounded up to a supported whole MB.".into(),
                    );
                }
            }
            "format" => ensure!(value == "json", "Only gRPCurl JSON messages are supported."),
            "ignore" => {}
            _ => data[key] = json!(value),
        }
    }
    if !protos.is_empty() {
        ensure!(
            text(&data, "descriptor_file").is_empty(),
            "Use either -proto or -protoset, not both."
        );
        ensure!(
            protos.iter().all(|p| p.ends_with(".proto")),
            "Local proto sources must have the .proto extension."
        );
        data["descriptor_file"] = json!(protos[0]);
        data["grpc_proto_files"] = json!(protos);
    }
    if !includes.is_empty() {
        data["grpc_include_paths"] = json!(includes);
    }
    authorize(&mut data, &mut metadata, true);
    data["metadata"] = json!(metadata);
    let values = serde_json::Deserializer::from_str(text(&data, "message"))
        .into_iter::<Value>()
        .collect::<serde_json::Result<Vec<_>>>();
    if let Ok(values) = values {
        if values.len() > 1 {
            data["message"] = json!(serde_json::to_string_pretty(&values)?);
        }
    }
    if text(&data, "tls_cert_file").is_empty() != text(&data, "tls_key_file").is_empty() {
        notes.push("Select both the client certificate and its PEM private key in Settings before sending.".into());
    }
    Ok(Imported { data, notes })
}
pub fn parse(source: &str, shell: &str) -> Result<Imported> {
    let tokens = split_command(source, shell)?;
    let executable = tokens
        .first()
        .context("Paste a cURL or gRPCurl command.")?
        .replace('\\', "/");
    let name = executable.rsplit('/').next().unwrap().to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    let mut imported = match name {
        "curl" => curl(&tokens[1..])?,
        "grpcurl" => grpcurl(&tokens[1..])?,
        _ => bail!("The command must start with curl or grpcurl."),
    };
    if tokens[1..].iter().any(|t| {
        t.as_bytes()
            .windows(2)
            .any(|p| p[0] == b'$' && (p[1].is_ascii_alphabetic() || [b'_', b'{'].contains(&p[1])))
    }) {
        imported.notes.push("Shell variables are kept literally. Replace them with values or Reqon {{variables}} before sending.".into());
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_019_command_import_contracts() {
        let contracts: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/command-import-019.json"))
                .unwrap();
        for case in contracts {
            let command = case["command"].as_str().unwrap();
            let result = parse(command, case["shell"].as_str().unwrap());
            // Reqon now also imports proxies, which the 0.19 parser rejected.
            if command == "curl https://example.test --proxy http://proxy.test" {
                assert_eq!(result.unwrap().data["http_proxy"], "http://proxy.test");
                continue;
            }
            if case["rejected"] == true {
                assert!(result.is_err(), "{command}");
                continue;
            }
            let result = result.unwrap_or_else(|e| panic!("{command}: {e:#}"));
            assert_eq!(result.data, case["data"], "{command}");
            assert_eq!(json!(result.notes), case["notes"], "{command}");
        }
    }
}
