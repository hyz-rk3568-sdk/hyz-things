#!/usr/bin/env python3
from pathlib import Path


MAKEFILE = Path("Makefile")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    if old not in text:
        raise SystemExit(f"{label}: expected source block not found")
    return text.replace(old, new, 1)


def main() -> None:
    text = MAKEFILE.read_text()

    text = replace_once(
        text,
        """\tgrep -q 'portal-camera-tab' \"$(THINGS_APP)/src/web/main.rs\"\n\tgrep -q '免登录实时查看摄像头画面' \"$(THINGS_APP)/src/web/main.rs\"""",
        """\tgrep -q 'app-camera-tab' \"$(THINGS_APP)/src/web/app.rs\"\n\tgrep -q '\\[function_component(CameraLiveView)\\]' \"$(THINGS_APP)/src/web/pages/camera.rs\"""",
        "camera contract",
    )

    text = replace_once(
        text,
        """\tgrep -q 'default-brightness-level = <0>' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
\t! grep -qE '&(dsi1|dsi1_panel|backlight1)[[:space:]]*\\{[[:space:]]*status = \"disabled\"' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts""",
        """ifneq ($(wildcard sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts),)
\tgrep -q 'default-brightness-level = <0>' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
\t! grep -qE '&(dsi1|dsi1_panel|backlight1)[[:space:]]*\\{[[:space:]]*status = \"disabled\"' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
else
\t@printf '%s\\n' 'SKIP: SDK display static checks (SDK tree is not present)'
endif""",
        "display SDK guard",
    )

    text = replace_once(
        text,
        """\tgrep -q 'GST_VIDEO_COLOR_RANGE_0_255' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'MPP_FRAME_RANGE_JPEG' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'mpp_enc_cfg_set_s32 (self->mpp_cfg, \"prep:range\", range)' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'failed to set input color range' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'self->prop_dirty = TRUE' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c""",
        """ifneq ($(wildcard sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c),)
\tgrep -q 'GST_VIDEO_COLOR_RANGE_0_255' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'MPP_FRAME_RANGE_JPEG' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'mpp_enc_cfg_set_s32 (self->mpp_cfg, \"prep:range\", range)' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'failed to set input color range' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
\tgrep -q 'self->prop_dirty = TRUE' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
else
\t@printf '%s\\n' 'SKIP: SDK GStreamer static checks (SDK tree is not present)'
endif""",
        "GStreamer SDK guard",
    )

    guard_marker = "SKIP: SDK Buildroot/kernel/manifest static checks"
    if guard_marker not in text:
        start_marker = "\tsh -n sdk/buildroot/board/rockchip/hyz_things/post-build.sh\n"
        end_marker = (
            "\ttest \"$$(grep -cE 'revision=\\\"[0-9a-f]{40}\\\"' "
            "sdk/.repo/manifests/hyz-things-release.xml)\" -eq 20\n"
        )
        start = text.find(start_marker)
        if start == -1:
            raise SystemExit("SDK static block start not found")
        end_start = text.find(end_marker, start)
        if end_start == -1:
            raise SystemExit("SDK static block end not found")
        end = end_start + len(end_marker)
        block = text[start:end]
        guarded = (
            "ifneq ($(wildcard sdk/buildroot/board/rockchip/hyz_things/post-build.sh),)\n"
            + block
            + "else\n"
            + "\t@printf '%s\\n' 'SKIP: SDK Buildroot/kernel/manifest static checks (SDK tree is not present)'\n"
            + "endif\n"
        )
        text = text[:start] + guarded + text[end:]

    MAKEFILE.write_text(text)


if __name__ == "__main__":
    main()
