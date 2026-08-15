//! 摄像头时间戳水印配置。
//!
//! 水印是烧进编码视频的固定产品特性：日期 + 时间、左上角、黑底。字号是固定像素
//! 值，与分辨率无关，保证 720p 与 4K 等所有预设下文字大小一致。

/// strftime 风格格式，由 GStreamer `clockoverlay` 逐秒渲染。
pub const WATERMARK_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
/// 目标 rootfs 安装的 DejaVu Sans 字体族名（fontconfig 直接按族名匹配）。
pub const WATERMARK_FONT_FAMILY: &str = "DejaVu Sans";
/// 所有预设共用的固定字号（像素）。用显式 `px` 传给 pango，避免点距/DPI 歧义。
pub const WATERMARK_FONT_SIZE: u32 = 20;
pub const WATERMARK_FONT_SIZE_MIN: u32 = 16;
pub const WATERMARK_FONT_SIZE_MAX: u32 = 128;
pub const WATERMARK_PADDING: u32 = 8;
pub const WATERMARK_MAX_PADDING: u32 = 64;
pub const WATERMARK_MAX_FORMAT_BYTES: usize = 64;

/// 水印必须同时包含日期与时间 token，否则不满足"时间戳"语义。
const WATERMARK_DATE_TOKENS: [&[u8]; 7] = [b"%Y", b"%y", b"%m", b"%d", b"%F", b"%D", b"%x"];
const WATERMARK_TIME_TOKENS: [&[u8]; 9] = [
    b"%H", b"%I", b"%M", b"%S", b"%R", b"%T", b"%X", b"%r", b"%p",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkPosition {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

impl WatermarkPosition {
    pub const ALL: [Self; 4] = [
        Self::BottomRight,
        Self::BottomLeft,
        Self::TopRight,
        Self::TopLeft,
    ];

    pub const fn halign(self) -> &'static str {
        match self {
            Self::BottomRight | Self::TopRight => "right",
            Self::BottomLeft | Self::TopLeft => "left",
        }
    }

    pub const fn valign(self) -> &'static str {
        match self {
            Self::BottomRight | Self::BottomLeft => "bottom",
            Self::TopRight | Self::TopLeft => "top",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimestampWatermark {
    pub time_format: &'static str,
    pub position: WatermarkPosition,
    pub font_size: u32,
    pub shaded_background: bool,
    pub padding: u32,
}

impl TimestampWatermark {
    /// 固定产品水印，四种预设共用同一配置。
    pub const DEFAULT: Self = Self {
        time_format: WATERMARK_TIME_FORMAT,
        position: WatermarkPosition::TopLeft,
        font_size: WATERMARK_FONT_SIZE,
        shaded_background: true,
        padding: WATERMARK_PADDING,
    };

    pub fn validate(self) -> Result<(), WatermarkError> {
        let bytes = self.time_format.as_bytes();
        if bytes.is_empty() || bytes.len() > WATERMARK_MAX_FORMAT_BYTES {
            return Err(WatermarkError::InvalidTimeFormat);
        }
        if bytes[bytes.len() - 1] == b'%' {
            return Err(WatermarkError::InvalidTimeFormat);
        }
        let mut has_date = false;
        let mut has_time = false;
        let mut index = 0;
        while index + 1 < bytes.len() {
            if bytes[index] == b'%' {
                let token = &bytes[index..index + 2];
                if WATERMARK_DATE_TOKENS.contains(&token) {
                    has_date = true;
                }
                if WATERMARK_TIME_TOKENS.contains(&token) {
                    has_time = true;
                }
                index += 2;
            } else {
                index += 1;
            }
        }
        if !has_date || !has_time {
            return Err(WatermarkError::InvalidTimeFormat);
        }
        if !(WATERMARK_FONT_SIZE_MIN..=WATERMARK_FONT_SIZE_MAX).contains(&self.font_size) {
            return Err(WatermarkError::InvalidFontSize);
        }
        if self.padding > WATERMARK_MAX_PADDING {
            return Err(WatermarkError::InvalidPadding);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WatermarkError {
    #[error("timestamp watermark format must contain a date and a time token")]
    InvalidTimeFormat,
    #[error("timestamp watermark font size is out of range")]
    InvalidFontSize,
    #[error("timestamp watermark padding is out of range")]
    InvalidPadding,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_watermark_is_top_left_shaded_date_and_time() {
        let watermark = TimestampWatermark::DEFAULT;
        assert_eq!(watermark.time_format, "%Y-%m-%d %H:%M:%S");
        assert_eq!(watermark.position, WatermarkPosition::TopLeft);
        assert_eq!(watermark.font_size, WATERMARK_FONT_SIZE);
        assert!(watermark.shaded_background);
        assert_eq!(watermark.padding, WATERMARK_PADDING);
        assert_eq!(watermark.validate(), Ok(()));
    }

    #[test]
    fn watermark_font_is_fixed_at_the_product_size() {
        assert_eq!(WATERMARK_FONT_SIZE, 20);
        assert_eq!(TimestampWatermark::DEFAULT.font_size, WATERMARK_FONT_SIZE);
        assert_eq!(TimestampWatermark::DEFAULT.validate(), Ok(()));
    }

    #[test]
    fn position_halign_valign_pairs_cover_four_corners() {
        assert_eq!(WatermarkPosition::ALL.len(), 4);
        assert_eq!(WatermarkPosition::BottomRight.halign(), "right");
        assert_eq!(WatermarkPosition::BottomRight.valign(), "bottom");
        assert_eq!(WatermarkPosition::BottomLeft.halign(), "left");
        assert_eq!(WatermarkPosition::BottomLeft.valign(), "bottom");
        assert_eq!(WatermarkPosition::TopRight.halign(), "right");
        assert_eq!(WatermarkPosition::TopRight.valign(), "top");
        assert_eq!(WatermarkPosition::TopLeft.halign(), "left");
        assert_eq!(WatermarkPosition::TopLeft.valign(), "top");
    }

    #[test]
    fn time_format_requires_both_date_and_time_tokens() {
        for format in [
            "%Y-%m-%d",
            "%H:%M:%S",
            "",
            "no tokens",
            "%M",
            "%Y-",
            "ends with %",
        ] {
            let watermark = TimestampWatermark {
                time_format: format,
                ..TimestampWatermark::DEFAULT
            };
            assert_eq!(watermark.validate(), Err(WatermarkError::InvalidTimeFormat));
        }
        for format in [
            "%Y-%m-%d %H:%M:%S",
            "%F %T",
            "%y/%m/%d %R",
            "%x %X",
            "%d-%m-%Y %I:%M %p",
            "%Y%M",
        ] {
            let watermark = TimestampWatermark {
                time_format: format,
                ..TimestampWatermark::DEFAULT
            };
            assert_eq!(watermark.validate(), Ok(()));
        }
    }

    #[test]
    fn font_size_and_padding_are_bounded() {
        for font_size in [WATERMARK_FONT_SIZE_MIN - 1, WATERMARK_FONT_SIZE_MAX + 1] {
            let watermark = TimestampWatermark {
                font_size,
                ..TimestampWatermark::DEFAULT
            };
            assert_eq!(watermark.validate(), Err(WatermarkError::InvalidFontSize));
        }
        let watermark = TimestampWatermark {
            padding: WATERMARK_MAX_PADDING + 1,
            ..TimestampWatermark::DEFAULT
        };
        assert_eq!(watermark.validate(), Err(WatermarkError::InvalidPadding));
    }
}
