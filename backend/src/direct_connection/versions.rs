// Adapted from majsoul-ws-login (Apache-2.0); uses the current official Unity resource layout.
use super::{fetch_document, Region};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;

const RESOURCE_HOSTS: &[&str] = &["appstatic.mahjongsoul.com", "appstaticbk.mahjongsoul.com"];
// shortcut: Supports current v4 BundleInfoSO and LZ4 formats; update if the official format changes.
const LIMIT: usize = 8 * 1024 * 1024;

fn official_url(url: &str, hosts: &[&str]) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(url)?;
    ensure!(
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.host_str().is_some_and(|host| hosts.contains(&host)),
        "客户端更新地址不是可信官方地址"
    );
    Ok(url)
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .with_context(|| format!("客户端更新配置缺少 {name}"))
}

async fn get(http: &reqwest::Client, url: reqwest::Url) -> Result<Vec<u8>> {
    fetch_document(
        http.get(url).query(&[(
            "randv",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
                .to_string(),
        )]),
        LIMIT,
    )
    .await
}

fn warehouse_url(config: &Value, path: &str, hosts: &[&str]) -> Result<reqwest::Url> {
    let entries = config["urls"]
        .as_array()
        .context("客户端更新配置缺少 CDN 列表")?;
    let entry = entries
        .iter()
        .max_by_key(|entry| {
            (
                entry["Priority"].as_i64().unwrap_or_default(),
                entry["weight"].as_i64().unwrap_or_default(),
            )
        })
        .context("客户端更新 CDN 列表为空")?;
    let base = official_url(field(entry, "url")?, hosts)?;
    official_url(base.join(field(config, path)?)?.as_str(), hosts)
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    ensure!(len <= input.len(), "版本资源包被截断");
    let (head, tail) = input.split_at(len);
    *input = tail;
    Ok(head)
}

fn u32_be(input: &mut &[u8]) -> Result<usize> {
    Ok(u32::from_be_bytes(take(input, 4)?.try_into()?) as usize)
}

fn unpack(bytes: &[u8], size: usize, flags: usize) -> Result<Vec<u8>> {
    ensure!(size <= LIMIT, "资源包解压大小超限");
    let data = match flags & 0x3f {
        0 => bytes.to_vec(),
        2 | 3 => lz4_flex::block::decompress(bytes, size)?,
        other => bail!("不支持的 资源包压缩格式 {other}"),
    };
    ensure!(data.len() == size, "资源包解压大小不符");
    Ok(data)
}

fn bundle_data(bundle: &[u8]) -> Result<Vec<u8>> {
    let mut input = bundle;
    ensure!(take(&mut input, 8)? == b"UnityFS\0", "版本资源包格式无效");
    let format = u32_be(&mut input)?;
    ensure!((6..=8).contains(&format), "不支持的 UnityFS 格式 {format}");
    for _ in 0..2 {
        let len = input
            .iter()
            .position(|&b| b == 0)
            .context("资源包缺少引擎版本")?
            + 1;
        take(&mut input, len)?;
    }
    let size = u64::from_be_bytes(take(&mut input, 8)?.try_into()?);
    ensure!(size == bundle.len() as u64, "资源包长度不符");
    let compressed = u32_be(&mut input)?;
    let clear = u32_be(&mut input)?;
    let flags = u32_be(&mut input)?;
    ensure!(
        flags & !(0x3f | 0x40 | 0x80 | 0x200) == 0,
        "不支持的 资源包标志"
    );
    if format >= 7 {
        let padding = (16 - (bundle.len() - input.len()) % 16) % 16;
        take(&mut input, padding)?;
    }
    let mut offset = bundle.len() - input.len();
    let info = if flags & 0x80 != 0 {
        ensure!(compressed <= input.len(), "资源包索引越界");
        let end = input.len() - compressed;
        let bytes = input[end..].to_vec();
        input = &input[..end];
        bytes
    } else {
        offset += compressed;
        take(&mut input, compressed)?.to_vec()
    };
    let info = unpack(&info, clear, flags)?;
    let mut blocks = info.as_slice();
    take(&mut blocks, 16)?;
    let count = u32_be(&mut blocks)?;
    ensure!(count <= blocks.len() / 10, "资源包块数无效");
    if flags & 0x200 != 0 {
        let padding = (16 - offset % 16) % 16;
        take(&mut input, padding)?;
    }
    let mut data = Vec::new();
    for _ in 0..count {
        let clear = u32_be(&mut blocks)?;
        let compressed = u32_be(&mut blocks)?;
        let flags = u16::from_be_bytes(take(&mut blocks, 2)?.try_into()?) as usize;
        ensure!(
            data.len().saturating_add(clear) <= LIMIT,
            "资源包解压总大小超限"
        );
        data.extend(unpack(take(&mut input, compressed)?, clear, flags)?);
    }
    Ok(data)
}

fn resource_from_bundle(bundle: &[u8], platform: &str) -> Result<String> {
    let data = bundle_data(bundle)?;
    // Only read the version TextAsset, not a general Unity object tree.
    let marker = b"\x07\x00\x00\x00version\0";
    let mut found = None;
    for start in data
        .windows(marker.len())
        .enumerate()
        .filter_map(|(i, b)| (b == marker).then_some(i))
    {
        let mut text = &data[start + marker.len()..];
        let len = u32::from_le_bytes(take(&mut text, 4)?.try_into()?) as usize;
        let version: Value = serde_json::from_slice(take(&mut text, len)?)?;
        if version["platform"] != platform {
            continue;
        }
        let resource = field(&version, "version")?;
        ensure!(valid_version(resource), "resource 格式无效");
        ensure!(found.is_none(), "resource 声明不唯一");
        found = Some(resource.to_owned());
    }
    found.with_context(|| format!("资源包缺少 {platform} resource"))
}

fn u32_le(input: &mut &[u8]) -> Result<usize> {
    Ok(u32::from_le_bytes(take(input, 4)?.try_into()?) as usize)
}

fn unity_string<'a>(input: &mut &'a [u8], limit: usize) -> Result<&'a str> {
    let len = u32_le(input)?;
    ensure!(len <= limit, "资源清单字符串过长");
    let text = std::str::from_utf8(take(input, len)?)?;
    take(input, (4 - len % 4) % 4)?;
    Ok(text)
}

fn web_version_bundle(manifest: &[u8]) -> Result<String> {
    let data = bundle_data(manifest)?;
    let marker = b"\x0c\0\0\0BundleInfoSO";
    let mut found = None;
    // MonoScript also contains this name; require the bundle/asset arrays of the MonoBehaviour.
    for (start, _) in data
        .windows(marker.len())
        .enumerate()
        .filter(|(_, b)| *b == marker)
    {
        if let Ok(name) = web_info(&data[start + marker.len()..]) {
            ensure!(found.is_none(), "官方资源清单的版本映射不唯一");
            found = Some(name);
        }
    }
    found.context("官方资源清单缺少有效的 BundleInfoSO 版本映射")
}

fn web_info(mut input: &[u8]) -> Result<String> {
    let count = u32_le(&mut input)?;
    ensure!(count > 0 && count <= input.len() / 8, "资源清单包数量无效");
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        names.push(unity_string(&mut input, 256)?);
        take(&mut input, 4)?; // fileSize
    }
    let count = u32_le(&mut input)?;
    ensure!(count <= input.len() / 8, "资源清单文件数量无效");
    let mut found = None;
    for _ in 0..count {
        let path = unity_string(&mut input, 4096)?;
        let index = u32_le(&mut input)?;
        if path != "MyAssets/docs_version/version.json" {
            continue;
        }
        ensure!(found.is_none(), "资源清单中的 version.json 映射不唯一");
        let name = names.get(index).context("version.json 的资源包索引无效")?;
        ensure!(
            !name.is_empty()
                && *name != "."
                && *name != ".."
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._$@-".contains(&b)),
            "版本资源包名称无效"
        );
        found = Some((*name).to_owned());
    }
    found.context("官方资源清单缺少 MyAssets/docs_version/version.json")
}

async fn bundle_base(
    http: &reqwest::Client,
    settings: reqwest::Url,
    hosts: &[&str],
) -> Result<reqwest::Url> {
    let settings: Value = serde_json::from_slice(&get(http, settings).await?)?;
    let warehouse = settings["warehouses"]
        .as_array()
        .and_then(|entries| entries.first())
        .context("官方更新配置缺少资源仓库")?;
    let warehouse: Value = serde_json::from_slice(
        &get(
            http,
            warehouse_url(warehouse, "warehouseSettingPath", hosts)?,
        )
        .await?,
    )?;
    let base = warehouse_url(&warehouse, "bundlePath", hosts)?;
    ensure!(base.path().ends_with('/'), "资源仓库路径缺少结尾斜线");
    Ok(base)
}

fn web_settings(region: Region) -> String {
    // These entrypoints come from the official Web GameLoader, not a version snapshot.
    match region {
        Region::Cn => {
            "https://game.maj-soul.com/assetbundles/clientBundleSettings/chs_t-release.json".into()
        }
        _ => format!(
            "https://appstatic.mahjongsoul.com/v4/{0}/clientbundlesettings/{0}-release.json",
            region.tag()
        ),
    }
}

async fn fetch_web_resource(http: &reqwest::Client, region: Region) -> Result<String> {
    let hosts: &[&str] = if region == Region::Cn {
        &["game.maj-soul.com"]
    } else {
        RESOURCE_HOSTS
    };
    let settings = official_url(&web_settings(region), hosts)?;
    let base = bundle_base(http, settings, hosts).await?;
    let prefix = if region == Region::Cn {
        "/assetbundles/".into()
    } else {
        format!("/v4/{}/resources/", region.tag())
    };
    ensure!(base.path().starts_with(&prefix), "资源仓库与所选区服不匹配");
    let base = base.join(if region == Region::Cn {
        "ASTC/"
    } else {
        "WebGL/ASTC/"
    })?;
    let manifest = get(http, base.join("bundle_info_so.majset")?).await?;
    let name = web_version_bundle(&manifest)?;
    let bundle = get(http, base.join(&name)?).await?;
    resource_from_bundle(&bundle, "WebGL")
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    version.len() <= 32
        && parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}

pub(super) fn valid_client_version(version: &str) -> bool {
    version
        .split_once("_2022-")
        .is_some_and(|(platform, resource)| {
            matches!(platform, "WebGL" | "StandaloneWindows64") && valid_version(resource)
        })
}

fn package_from_html(html: &str) -> Result<String> {
    let mut found = None;
    for (index, _) in html.match_indices("productVersion") {
        let previous = html[..index].chars().next_back();
        if previous.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let mut rest = &html[index + "productVersion".len()..];
        if let Some(quote @ ('\'' | '"')) = previous {
            let Some(suffix) = rest.strip_prefix(quote) else {
                continue;
            };
            rest = suffix;
        }
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote @ ('\'' | '"')) = rest.chars().next() else {
            continue;
        };
        let Some((version, _)) = rest[1..].split_once(quote) else {
            continue;
        };
        if !valid_version(version) {
            continue;
        }
        ensure!(
            found.is_none_or(|previous| previous == version),
            "conflicting official product versions"
        );
        found = Some(version);
    }
    found
        .map(str::to_owned)
        .context("official product version missing")
}

pub(super) async fn package(http: &reqwest::Client, region: Region) -> Result<String> {
    let html = fetch_document(http.get(region.web_page()), 512 * 1024).await?;
    package_from_html(std::str::from_utf8(&html).context("official page is not UTF-8")?)
}

pub(super) async fn fetch(http: &reqwest::Client, region: Region) -> Result<(String, String)> {
    tokio::time::timeout(std::time::Duration::from_secs(40), async {
        tokio::try_join!(package(http, region), fetch_web_resource(http, region))
    })
    .await
    .context("client version discovery timed out")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_bundle_fixtures_decode_and_reject_truncation() {
        let manifest = include_bytes!("../../tests/fixtures/direct-cn-web-manifest.bundle");
        let version = include_bytes!("../../tests/fixtures/direct-cn-web-version.bundle");
        assert_eq!(
            web_version_bundle(manifest).unwrap(),
            "u6v_td1bd8bb002s_a8a5b47edee1b8ac51a4.majset"
        );
        assert_eq!(resource_from_bundle(version, "WebGL").unwrap(), "0.16.289");
        assert!(resource_from_bundle(version, "StandaloneWindows64").is_err());
        for end in [0, 7, 50, version.len() - 1] {
            assert!(resource_from_bundle(&version[..end], "WebGL").is_err());
        }
        assert!(web_version_bundle(&manifest[..manifest.len() - 1]).is_err());
        assert!(unpack(&[], LIMIT + 1, 3).is_err());
    }

    #[test]
    fn versions_and_cdn_addresses_are_validated() {
        assert_eq!(
            package_from_html("productVersion: '4.0.18'").unwrap(),
            "4.0.18"
        );
        assert!(package_from_html("productVersion: '4.0.18', productVersion: '4.0.19'").is_err());
        assert!(package_from_html("otherproductVersion: '4.0.18'").is_err());
        assert!(valid_client_version("StandaloneWindows64_2022-0.16.272"));
        assert!(!valid_client_version("StandaloneWindows64_2022-0.16.x"));
        assert!(!valid_client_version("Android_2022-0.16.272"));
        for url in [
            "http://appstatic.mahjongsoul.com/file",
            "https://appstatic.mahjongsoul.com.evil.test/file",
            "https://user@appstatic.mahjongsoul.com/file",
            "https://appstatic.mahjongsoul.com:8443/file",
        ] {
            assert!(official_url(url, RESOURCE_HOSTS).is_err());
        }
    }

    #[tokio::test]
    #[ignore = "queries public official version files and routes without login credentials"]
    async fn public_regions_discover_versions_and_routes() {
        let http = super::super::http_client().unwrap();
        for region in [Region::Cn, Region::Jp, Region::En, Region::Kr] {
            let (package, resource) = fetch(&http, region).await.unwrap();
            assert!(valid_version(&package) && valid_version(&resource));
            let routes = super::super::fetch_routes(&http, region, &package, "Web")
                .await
                .unwrap();
            assert!(!routes.is_empty());
        }
    }
}
