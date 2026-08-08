# hyz-ota

Minimal RK3568 OTA client for the bring-up phase. It downloads firmware over
HTTP(S), verifies a caller-provided SHA-256 digest, validates the Rockchip
firmware header, and hands the image to Rockchip `updateEngine`.

This is not yet a production-safe A/B updater. Production releases must add
signed metadata, rollback protection, power-loss testing, and an A/B or recovery
strategy.
