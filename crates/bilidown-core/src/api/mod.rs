//! B 站 API 层：纯逻辑，不依赖 egui / Tauri（可单测）。
//!
//! 模块：
//! - [`client`]：HTTP 客户端（cookie、超时、代理、WBI key 缓存）
//! - [`error`]：结构化错误 [`error::BiliError`] / [`error::ErrorKind`]
//! - [`types`]：领域类型与 playurl 数据模型
//! - [`wbi`]：WBI 签名
//! - [`parse`]：输入解析（链接/裸 ID/短链/分 P 提示）
//! - [`video`] / [`audio`]：内容解析（含 UGC 合集、
//!   歌单分页）
//! - [`source`]：列表型内容源统一模型（[`source::SourceItem`]）与粘贴链接识别
//! - [`login`] / [`account`]：扫码登录、凭据恢复/续期/退出
//! - [`pcdn`]：PCDN 直链过滤
//! - [`subtitle`]：字幕列表/JSON → SRT（可选后处理，change add-download-features D2）

pub mod account;
pub mod audio;
pub mod client;
pub mod error;
pub mod login;
pub mod parse;
pub mod pcdn;
pub mod source;
pub mod subtitle;
pub mod types;
pub mod video;
pub mod wbi;

// TODO(follow-up，非本 change 范围)：弹幕/NFO 等后处理；内容源扩展见
// add-download-features tasks 5.x（core 侧提供 UGC 合集/歌单共用列表模型）。
