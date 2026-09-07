use std::path::Path;
use std::time::Duration;

pub(super) async fn wait(primary: &str) {
    if let Ok(spec) = std::env::var("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND")
        && let Some((command, path)) = spec.split_once('=')
        && command == primary
    {
        let gate = Path::new(path);
        std::fs::write(gate.with_extension("entered"), primary)
            .expect("write side-effect gate entry marker");
        // Bound cleanup after a failed test without letting machine speed
        // decide whether another family's attribution overlaps this effect.
        tokio::time::timeout(Duration::from_secs(60), async {
            while gate.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("test did not release its side-effect gate");
    }

    if let Ok(spec) = std::env::var("GIT_AI_TEST_DELAY_SIDE_EFFECT_MS_FOR_COMMAND") {
        for entry in spec.split(',') {
            let Some((command, delay_ms)) = entry.split_once('=') else {
                continue;
            };
            if command == primary
                && let Ok(delay_ms) = delay_ms.parse::<u64>()
                && delay_ms > 0
            {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                break;
            }
        }
    }
}
