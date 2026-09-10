#![cfg(feature = "web")]

mod api;
mod app;
mod ui;

mod hooks;
mod pages;

use api::*;
use app::*;
use hooks::*;
use pages::*;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use hyz_things::domain::{
    camera::{
        CameraAccessKind, CameraErrorCategory, CameraPipelineState, CameraRotation, CameraStatus,
        CameraStreamPreset,
    },
    panel::{
        DisplayRequest, PanelBootstrap, ProxyDelayRefreshRequest, ProxyGroup, ProxySelectionRequest,
    },
    status::{
        Component, ComponentState, LanTunEffective, LocalSystemProxyEffective, ProxyResourceState,
        ProxyStatus, SnapshotState, StatusSnapshot, TailscaleConnectionType,
        TailscaleErrorCategory, TailscaleStatus, UplinkId, UplinkStatus,
    },
    tailscale::{
        TailscaleBackendState, TailscaleMode, TailscalePeer, TailscalePeerConnection,
        TailscalePeerSnapshot,
    },
};
use js_sys::{Date, Function, Object, Promise, Reflect};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AudioContext, CanvasRenderingContext2d, Document, Element, Event, HtmlCanvasElement,
    HtmlElement, HtmlInputElement, HtmlMediaElement, HtmlSelectElement, HtmlVideoElement,
    KeyboardEvent, MediaStream, MediaStreamAudioDestinationNode, MediaStreamConstraints,
    MediaStreamTrack, MediaTrackConstraints, PointerEvent, RequestCredentials,
    RtcIceGatheringState, RtcPeerConnection, RtcPeerConnectionState, RtcRtpSender,
    RtcRtpTransceiverDirection, RtcRtpTransceiverInit, RtcSdpType, RtcSessionDescriptionInit,
    RtcTrackEvent, Storage, VideoFrame, VideoFrameInit, Window,
};
use yew::prelude::*;

use ui::*;

const CAMERA_ICE_GATHER_TIMEOUT_MS: u32 = 10_000;
const CAMERA_ICE_POLL_MS: u32 = 50;
// 页面隐藏后不立即关闭直播会话：宽限期内回来就继续，超时才真正关闭
// （多 viewer 场景下避免切走标签页即断流）。
const CAMERA_HIDDEN_CLOSE_GRACE_MS: u32 = 60_000;
const POLL_DELAY_MS: u32 = 2_000;
const NETWORK_APPLY_PAINT_DELAY_MS: u32 = 150;
// 画面设置（预设/旋转）提交遇到 camera busy（残留会话或 close 尚未完成）时
// 短暂重试，避免连续快速操作被一个过渡态永久卡在"未播放"。
const CAMERA_UPDATE_RETRY_ATTEMPTS: u8 = 3;
const CAMERA_UPDATE_RETRY_DELAY_MS: u32 = 1_000;
const MISSING: &str = "—";
fn is_escape_key(event: &KeyboardEvent) -> bool {
    matches!(event.key().as_str(), "Escape" | "Esc") || event.code() == "Escape"
}

impl WifiCountryDto {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Au => "AU",
            Self::Br => "BR",
            Self::Ca => "CA",
            Self::Cn => "CN",
            Self::De => "DE",
            Self::Fr => "FR",
            Self::Gb => "GB",
            Self::In => "IN",
            Self::Jp => "JP",
            Self::Kr => "KR",
            Self::Nz => "NZ",
            Self::Sg => "SG",
            Self::Tw => "TW",
            Self::Us => "US",
        }
    }
}

impl NetworkApplyIntent {
    const fn confirmation(&self) -> (&'static str, &'static str) {
        match self {
            Self::Sta(_) => (
                "应用上游 Wi-Fi？",
                "STA 切换可能让 AP 跟随新信道并短暂断开管理连接。确认后弹窗会先关闭，再开始应用。",
            ),
            Self::Ap => (
                "应用下游 AP？",
                "当前 AP 会立即断开。确认后弹窗会先关闭，请随后连接新的 AP，并在倒计时结束前确认保留。",
            ),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Good,
    Warn,
    Bad,
    Neutral,
}

impl Tone {
    fn class(self) -> &'static str {
        match self {
            Self::Good => "text-success",
            Self::Warn => "text-warning",
            Self::Bad => "text-error",
            Self::Neutral => "text-base-content/60",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{countdown_snapshot, wan_traffic, EXAM_COUNTDOWN_TARGETS};
    use hyz_things::domain::status::{Component, InterfaceStats, SystemStats};

    #[test]
    fn upcoming_exam_targets_are_sorted_chronologically() {
        let dates: Vec<_> = EXAM_COUNTDOWN_TARGETS
            .iter()
            .map(|target| target.target_iso)
            .collect();
        assert_eq!(
            dates,
            vec![
                "2026-11-29T00:00:00+08:00",
                "2026-12-06T00:00:00+08:00",
                "2027-03-14T00:00:00+08:00",
                "2027-03-27T00:00:00+08:00",
            ]
        );
    }

    #[test]
    fn countdown_snapshot_tracks_progress_and_clamps_after_exam_day() {
        let before = countdown_snapshot(0, 0, 1_000);
        assert_eq!(before.remaining_seconds, 1);
        assert_eq!(before.progress_percent, 0);
        assert!(!before.finished);

        let during = countdown_snapshot(250, 0, 1_000);
        assert_eq!(during.remaining_seconds, 1);
        assert_eq!(during.progress_percent, 25);
        assert!(!during.finished);

        let after = countdown_snapshot(1_500, 0, 1_000);
        assert_eq!(after.remaining_seconds, 0);
        assert_eq!(after.progress_percent, 100);
        assert!(after.finished);
    }

    #[test]
    fn wan_traffic_sums_ethernet_and_wifi_only() {
        let component = Component::available(SystemStats {
            interfaces: vec![
                InterfaceStats {
                    name: "eth0".to_owned(),
                    rx_bytes: 10,
                    tx_bytes: 20,
                },
                InterfaceStats {
                    name: "wlan0".to_owned(),
                    rx_bytes: 3,
                    tx_bytes: 5,
                },
                InterfaceStats {
                    name: "br-lan".to_owned(),
                    rx_bytes: 100,
                    tx_bytes: 200,
                },
            ],
            ..SystemStats::default()
        });

        assert_eq!(wan_traffic(&component), Some((13, 25)));
    }

    #[test]
    fn wan_traffic_is_missing_without_an_uplink_interface() {
        let component = Component::available(SystemStats {
            interfaces: vec![InterfaceStats {
                name: "br-lan".to_owned(),
                rx_bytes: 100,
                tx_bytes: 200,
            }],
            ..SystemStats::default()
        });

        assert_eq!(wan_traffic(&component), None);
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
