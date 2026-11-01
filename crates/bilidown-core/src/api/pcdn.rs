//! PCDN 直链归并。
//!
//! B 站 DASH 直链可能指向 PCDN 节点（速度不稳定、易挂起）。这里按查询参数
//! `os` 与 host 特征把候选 URL 归为「官方镜像 / UPOS / BCache / 其他」，
//! 优先返回官方镜像；没有镜像时把可归并的节点改写为官方镜像域名。
//! 逻辑来自对公开接口返回值的观察，测试固定了归并顺序与改写结果。

/// 官方镜像域名（按顺序轮换）。
const MIRRORS: [&str; 2] = [
    "upos-sz-mirrorali.bilivideo.com",
    "upos-sz-mirrorcos.bilivideo.com",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Mirror,
    Upos,
    Bcache,
    Other,
}

#[derive(Debug)]
struct Candidate<'a> {
    url: &'a str,
    host: &'a str,
    origin: Origin,
}

/// 拆出 `scheme://host` 与查询串里的 `os`；解析失败返回 None。
fn inspect(url: &str) -> Option<Candidate<'_>> {
    let scheme_end = url.find("://")?;
    let after_scheme = &url[scheme_end + 3..];
    let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let host = &after_scheme[..host_end];
    let host_plain = host.split(':').next().unwrap_or(host);
    let tail = &after_scheme[host_end..];

    let mut os = "";
    for segment in tail.split(['?', '&']) {
        if let Some(value) = segment.strip_prefix("os=") {
            os = value;
            break;
        }
    }

    let origin = if host_plain.contains("mirror") && os.ends_with("bv") {
        Origin::Mirror
    } else if os == "upos" {
        Origin::Upos
    } else if host_plain.starts_with("cn") && os == "bcache" {
        Origin::Bcache
    } else {
        Origin::Other
    };

    Some(Candidate { url, host: host_plain, origin })
}

fn replace_host(url: &str, host: &str, replacement: &str) -> String {
    if host.is_empty() {
        return url.to_string();
    }
    if let Some(pos) = url.find(host) {
        let mut out = String::with_capacity(url.len() + replacement.len());
        out.push_str(&url[..pos]);
        out.push_str(replacement);
        out.push_str(&url[pos + host.len()..]);
        out
    } else {
        url.to_string()
    }
}

/// 返回按「官方镜像优先」重排/改写后的 URL 列表。
pub fn filter_urls(urls: &[String]) -> Vec<String> {
    let candidates: Vec<Candidate<'_>> = urls.iter().filter_map(|u| inspect(u)).collect();
    let mirrors: Vec<&Candidate<'_>> = candidates.iter().filter(|c| c.origin == Origin::Mirror).collect();
    let upos: Vec<&Candidate<'_>> = candidates.iter().filter(|c| c.origin == Origin::Upos).collect();
    let bcache: Vec<&Candidate<'_>> = candidates.iter().filter(|c| c.origin == Origin::Bcache).collect();

    if !mirrors.is_empty() {
        let mut out: Vec<String> = mirrors.iter().map(|c| c.url.to_string()).collect();
        if out.len() < 2 {
            out.extend(upos.iter().map(|c| c.url.to_string()));
        }
        return out;
    }

    let rewrite_from: &[&Candidate<'_>] = if !upos.is_empty() { &upos } else { &bcache };
    if rewrite_from.is_empty() {
        return urls.to_vec();
    }
    rewrite_from
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            replace_host(candidate.url, candidate.host, MIRRORS[index % MIRRORS.len()])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_mirror() {
        let urls = vec![
            "https://mcdn.bilivideo.com/v.m4s?os=mcdn".to_string(),
            "https://upos-sz-mirror08h.bilivideo.com/v.m4s?os=uposbv".to_string(),
        ];
        let out = filter_urls(&urls);
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("mirror08h"));
    }

    #[test]
    fn replaces_upos_host_with_official_mirror() {
        let urls = vec!["https://xy123.above-c.mcdn.bilivideo.com/v.m4s?e=1&os=upos".to_string()];
        let out = filter_urls(&urls);
        assert_eq!(out.len(), 1);
        assert!(
            out[0].starts_with("https://upos-sz-mirrorali.bilivideo.com/"),
            "{}",
            out[0]
        );
        assert!(out[0].ends_with("v.m4s?e=1&os=upos"));
    }

    #[test]
    fn keeps_others_when_no_official() {
        let urls = vec!["https://p2p.example.com/v.m4s?os=abc".to_string()];
        let out = filter_urls(&urls);
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("p2p.example.com"));
    }

    #[test]
    fn bcache_rotation() {
        let urls = vec![
            "https://cn-xyz.example.com/a.m4s?os=bcache".to_string(),
            "https://cn-abc.example.com/b.m4s?os=bcache".to_string(),
        ];
        let out = filter_urls(&urls);
        assert!(out[0].starts_with("https://upos-sz-mirrorali.bilivideo.com/"));
        assert!(out[1].starts_with("https://upos-sz-mirrorcos.bilivideo.com/"));
    }
}
