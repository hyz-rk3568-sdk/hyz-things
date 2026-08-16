//! OTA command wire DTO. CLI parsing stays in the router package.

use serde::{Deserialize, Serialize};

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
