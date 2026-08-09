use serde::{Deserialize, Serialize};
use std::{error::Error, ffi::OsString, fmt};

pub const OTA_USAGE: &str = "Usage:\n  hyz-router ota verify <upgrade.fw> <sha256>\n  hyz-router ota download <source> <sha256>\n  hyz-router ota install <firmware.fw> <sha256> [--reboot]\n  hyz-router ota install-recovery <firmware.fw> <sha256> [--reboot]\n  hyz-router ota apply <source> <sha256> [--reboot]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OtaCommand {
    Verify {
        firmware: String,
        expected: String,
    },
    Download {
        source: String,
        expected: String,
    },
    Install {
        firmware: String,
        expected: String,
        reboot: bool,
    },
    InstallRecovery {
        firmware: String,
        expected: String,
        reboot: bool,
    },
    Apply {
        source: String,
        expected: String,
        reboot: bool,
    },
}

pub fn parse_ota_cli<I, S>(args: I) -> Result<OtaCommand, OtaCliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args = args
        .into_iter()
        .map(|argument| {
            argument
                .into()
                .into_string()
                .map_err(|_| OtaCliError::Usage)
        })
        .collect::<Result<Vec<_>, _>>()?;

    match args.as_slice() {
        [command, firmware, expected] if command == "verify" => Ok(OtaCommand::Verify {
            firmware: firmware.clone(),
            expected: expected.clone(),
        }),
        [command, source, expected] if command == "download" => Ok(OtaCommand::Download {
            source: source.clone(),
            expected: expected.clone(),
        }),
        [command, firmware, expected] if command == "install" => Ok(OtaCommand::Install {
            firmware: firmware.clone(),
            expected: expected.clone(),
            reboot: false,
        }),
        [command, firmware, expected, flag] if command == "install" && flag == "--reboot" => {
            Ok(OtaCommand::Install {
                firmware: firmware.clone(),
                expected: expected.clone(),
                reboot: true,
            })
        }
        [command, firmware, expected] if command == "install-recovery" => {
            Ok(OtaCommand::InstallRecovery {
                firmware: firmware.clone(),
                expected: expected.clone(),
                reboot: false,
            })
        }
        [command, firmware, expected, flag]
            if command == "install-recovery" && flag == "--reboot" =>
        {
            Ok(OtaCommand::InstallRecovery {
                firmware: firmware.clone(),
                expected: expected.clone(),
                reboot: true,
            })
        }
        [command, source, expected] if command == "apply" => Ok(OtaCommand::Apply {
            source: source.clone(),
            expected: expected.clone(),
            reboot: false,
        }),
        [command, source, expected, flag] if command == "apply" && flag == "--reboot" => {
            Ok(OtaCommand::Apply {
                source: source.clone(),
                expected: expected.clone(),
                reboot: true,
            })
        }
        _ => Err(OtaCliError::Usage),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtaCliError {
    Usage,
}

impl fmt::Display for OtaCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(OTA_USAGE)
    }
}

impl Error for OtaCliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_returns_data_only_and_rejects_extra_deployment_path() {
        let digest = "a".repeat(64);
        assert_eq!(
            parse_ota_cli([
                "download",
                "https://example.invalid/update.fw",
                digest.as_str(),
            ])
            .unwrap(),
            OtaCommand::Download {
                source: "https://example.invalid/update.fw".to_owned(),
                expected: "a".repeat(64),
            }
        );
        assert!(parse_ota_cli([
            "download",
            "https://example.invalid/update.fw",
            digest.as_str(),
            "/arbitrary/deployment.fw",
        ])
        .is_err());
    }

    #[test]
    fn inbound_cli_has_no_application_or_outbound_execution_surface() {
        let source = include_str!("ota_cli.rs");
        assert!(!source.contains(concat!("Firmware", "PlatformPort")));
        assert!(!source.contains(concat!("Ota", "Service")));
        assert!(!source.contains(concat!("adapters::", "outbound")));
    }
}
