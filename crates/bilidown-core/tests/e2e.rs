//! 端到端核心管线测试（走真实网络，默认忽略）。
//! 运行：cargo test -p bilidown-core -- --ignored

use bilidown_core::api::client::{BiliClient, download_client};

/// 本机 LuLu 会拦截未签名 dev 二进制的直连；把 BILIDOWN_TEST_PROXY 指向本机
/// HTTP 代理（如 http://127.0.0.1:7897）即可在测试环境放行。生产默认仍直连。
fn test_proxy() -> Option<String> {
    std::env::var("BILIDOWN_TEST_PROXY")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn test_client() -> BiliClient {
    BiliClient::new(test_proxy().as_deref(), Default::default()).expect("客户端")
}

fn test_client_authenticated() -> Option<BiliClient> {
    let path = std::env::var("BILIDOWN_TEST_CREDENTIALS").ok()?;
    let credentials = bilidown_core::storage::credentials::load(std::path::Path::new(&path));
    if !credentials.has_login() {
        return None;
    }
    let client = test_client();
    client.set_cookies(credentials.stored());
    client.apply_device(&credentials.device);
    Some(client)
}

fn test_http() -> reqwest::Client {
    let mut b = reqwest::Client::builder();
    b = match test_proxy() {
        Some(p) => b.proxy(reqwest::Proxy::all(p).expect("合法代理地址")),
        None => b.no_proxy(),
    };
    b.build().expect("http client")
}
use bilidown_core::api::parse::parse_one;
use bilidown_core::api::types::BiliId;
use bilidown_core::api::video::{play_url, resolve_content};
use bilidown_core::download::{DownloadProgress, DownloadRequest, download_stream};
use bilidown_core::media::ffmpeg::{Ffmpeg, MergeRequest, detect_ffmpeg};
use bilidown_core::media::quality::{StreamChoice, select};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[tokio::test]
#[ignore = "需要真实网络与本地 ffmpeg"]
async fn e2e_download_public_video_lowest() {
    let bili = test_client();
    bili.activate_buvid().await;

    let id = parse_one(&test_http(), "https://www.bilibili.com/video/BV1GJ411x7h7")
        .await
        .expect("解析 BV 链接");
    let content = resolve_content(&bili, &id).await.expect("view 接口");
    assert!(!content.title.is_empty());
    assert!(!content.entries.is_empty());

    let entry = &content.entries[0];
    let play = play_url(&bili, &id, entry, false)
        .await
        .expect("匿名 playurl");
    assert!(!play.qualities.is_empty(), "匿名也应至少有 360P/480P");

    // 选最低档（测试友好）
    let lowest = play.qualities.last().expect("有档位").quality;
    let selection = select(
        &play,
        StreamChoice {
            quality: Some(lowest),
            ..Default::default()
        },
        true,
    )
    .expect("可选流");
    let out_dir = std::env::temp_dir().join("bilidown-e2e");
    tokio::fs::create_dir_all(&out_dir).await.unwrap();
    let v_path = out_dir.join("e2e.v.m4s");
    let a_path = out_dir.join("e2e.a.m4s");
    let out = out_dir.join("e2e.mp4");
    let _ = tokio::fs::remove_file(&v_path).await;
    let _ = tokio::fs::remove_file(&a_path).await;
    let _ = tokio::fs::remove_file(&out).await;

    let cancel = tokio_util::sync::CancellationToken::new();
    let http = download_client(test_proxy().as_deref()).expect("下载 client");
    let progress = Arc::new(AtomicU64::new(0));
    let pv = Arc::clone(&progress);
    let urls_v = bilidown_core::api::pcdn::filter_urls(&selection.video_urls());
    let urls_a = bilidown_core::api::pcdn::filter_urls(&selection.audio_urls());
    let v_req = DownloadRequest::new(&http, &urls_v, &v_path, &cancel);
    let a_req = DownloadRequest::new(&http, &urls_a, &a_path, &cancel);
    let (v, a) = tokio::join!(
        download_stream(&v_req, |p: DownloadProgress| {
            pv.fetch_add(p.downloaded, Ordering::Relaxed);
        }),
        download_stream(&a_req, |_p: DownloadProgress| {}),
    );
    let vn = v.expect("视频流下载");
    let an = a.expect("音频流下载");
    assert!(vn > 100_000, "视频流过小: {vn}");
    assert!(an > 1_000, "音频流过小: {an}");
    assert!(progress.load(Ordering::Relaxed) > 0, "应有进度回调");

    let ffmpeg_path = detect_ffmpeg(None).expect("本机应有 ffmpeg");
    let ffmpeg = Ffmpeg::new(ffmpeg_path);
    let req = MergeRequest {
        video: v_path.clone(),
        audio: Some(a_path.clone()),
        out: out.clone(),
        title: Some("e2e".into()),
        container: "mp4".into(),
        total_us: None,
    };
    ffmpeg.merge(&req, &cancel, |_| {}).await.expect("合并");
    let meta = tokio::fs::metadata(&out).await.expect("产物存在");
    assert!(meta.len() > 100_000, "合并产物过小: {}", meta.len());
}

#[tokio::test]
#[ignore = "需要真实网络"]
async fn e2e_parse_ids_and_shorts() {
    let http = test_http();
    let bare = parse_one(&http, "BV1GJ411x7h7").await.expect("裸 BV");
    assert!(matches!(bare, BiliId::Video { .. }));
    let av = parse_one(&http, "av170001").await.expect("av 号");
    assert!(matches!(av, BiliId::Video { .. }));
    let bad = parse_one(&http, "https://example.com/xyz").await;
    assert!(bad.is_err());
}

#[tokio::test]
#[ignore = "需要真实网络"]
async fn e2e_audio_error_mapping() {
    use bilidown_core::api::audio;
    let bili = test_client();
    // 已下架音频（2026-10 实测 code 4511001）；匿名直链下载需登录，
    // 完整音频下载链路待登录态人工验收（tasks 8.2）
    let err = audio::resolve_song(&bili, "912772")
        .await
        .expect_err("应返回下架错误");
    let msg = err.describe();
    assert!(
        msg.contains("4511001") || msg.contains("未找到") || msg.contains("下架"),
        "错误应归一: {msg}"
    );
}

/// 番剧/课程：解析 season 与 playurl（PGC/PUGV 用 `result` 包络、ep_id 为纯数字、
/// 需要 season_id/avid），防止 B 站接口形态变化再次回归。
#[tokio::test]
#[ignore = "需要真实网络"]
async fn e2e_bangumi_and_cheese_playurl() {
    use bilidown_core::api::video::{play_url, resolve_content};

    let bili = test_client();
    let http = test_http();

    // 公开番剧（猫之茗）
    let ep = parse_one(&http, "https://www.bilibili.com/bangumi/play/ep425850")
        .await
        .expect("解析番剧 ep");
    let content = resolve_content(&bili, &ep).await.expect("番剧 season");
    assert!(!content.entries.is_empty());
    let first = &content.entries[0];
    assert!(first.season_id.is_some(), "PGC entry 应带 season_id");
    let play = play_url(&bili, &ep, first, false)
        .await
        .expect("番剧 playurl");
    assert!(!play.qualities.is_empty());

    // 课程（唐盾日语 免费试听）
    let cheese = parse_one(&http, "https://www.bilibili.com/cheese/play/ep812")
        .await
        .expect("解析课程 ep");
    let course = resolve_content(&bili, &cheese).await.expect("课程 season");
    assert!(!course.entries.is_empty());
    let first = &course.entries[0];
    assert!(
        first.season_id.is_some() && first.aid.is_some(),
        "PUGV entry 应有 season_id/aid"
    );
    let play = play_url(&bili, &cheese, first, false)
        .await
        .expect("课程 playurl");
    assert!(!play.qualities.is_empty());

    // ss 形式的课程也要能解析
    let ss = parse_one(&http, "https://www.bilibili.com/cheese/play/ss61")
        .await
        .expect("解析课程 ss");
    let course = resolve_content(&bili, &ss).await.expect("课程 ss season");
    assert!(!course.entries.is_empty());
}

#[tokio::test]
#[ignore = "需要真实网络"]
async fn e2e_multi_page_and_ugc_collection() {
    let bili = test_client();
    bili.activate_buvid().await;
    let http = test_http();

    let multi = parse_one(&http, "https://www.bilibili.com/video/BV1dw411s78P")
        .await
        .expect("解析多 P 视频");
    let content = resolve_content(&bili, &multi).await.expect("多 P 视频详情");
    assert_eq!(content.entries.len(), 30, "公开测试视频应有 30 个分 P");
    assert!(content.entries.iter().all(|entry| entry.cid > 0));

    let season = bilidown_core::api::video::resolve_ugc_season(&bili, "BV1t5smeKEeZ")
        .await
        .expect("UGC 合集详情")
        .expect("公开测试视频应属于 UGC 合集");
    assert!(season.title.contains("视频合集"));
    assert!(season.to_source_items().len() >= 2);
}

#[tokio::test]
#[ignore = "需要真实网络"]
async fn e2e_audio_menu_real() {
    use bilidown_core::api::audio;
    use bilidown_core::api::source::SourceKind;

    let bili = test_client();
    bili.activate_buvid().await;
    let mut menu = audio::menu_info(&bili, 106240).await.expect("歌单元信息");
    assert_eq!(menu.menu_id, 106240);
    assert!(!menu.title.is_empty());

    let page = audio::menu_page(&bili, 106240, 1, 20)
        .await
        .expect("歌单第一页");
    assert!(!page.songs.is_empty(), "公开歌单应至少有一首曲目");
    assert!(page.songs.iter().all(|song| song.song_id > 0));
    menu.songs = page.songs;

    let collection = menu.to_collection();
    assert_eq!(collection.kind, SourceKind::AudioList);
    assert!(!collection.items.is_empty());
    assert!(
        collection
            .items
            .iter()
            .all(|item| item.kind == SourceKind::Audio)
    );
}

#[tokio::test]
#[ignore = "需要真实网络与本地 ffmpeg"]
async fn e2e_subtitle_and_cover_download() {
    use bilidown_core::api::subtitle;
    use bilidown_core::media::cover;

    let bili = test_client_authenticated().unwrap_or_else(test_client);
    bili.activate_buvid().await;
    let http = test_http();

    let id = parse_one(&http, "https://www.bilibili.com/video/BV15b411L7M4")
        .await
        .expect("解析 TED 视频");
    let content = resolve_content(&bili, &id).await.expect("TED 视频详情");
    let entry = content.entries.first().expect("至少一个视频条目");
    let aid = entry.aid.expect("UGC 条目应有 aid");
    let tracks = subtitle::player_subtitles(&bili, aid, entry.cid)
        .await
        .expect("字幕列表");
    assert!(!tracks.is_empty(), "公开 TED 视频应至少有一条字幕");

    let srt = subtitle::download_srt(&bili, &tracks[0])
        .await
        .expect("字幕下载与 SRT 转换");
    assert!(srt.lines().any(|line| line.contains(" --> ")));
    assert!(srt.len() > 32);

    let cover_url = content.cover_url.as_deref().expect("视频应有封面");
    let dir = std::env::temp_dir().join("bilidown-e2e-subtitle-cover");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let video = dir.join("ted.mp4");
    let download_http = download_client(test_proxy().as_deref()).expect("下载 client");
    let cover_path = cover::download_cover(&download_http, cover_url, &video, &dir)
        .await
        .expect("封面下载");
    let meta = tokio::fs::metadata(cover_path).await.expect("封面文件");
    assert!(meta.len() > 1_000, "封面文件应非空");
}
