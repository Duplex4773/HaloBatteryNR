# Upstream behavior coverage

The pinned reference contains **459 upstream test IDs**. This inventory distinguishes full regression evidence from parser/helper coverage and retired Python implementation details. Mapped regression links are validated as actual Rust `#[test]` functions or native automation scripts containing failure assertions. Partial implementation pointers are explicitly labeled and do not count as regression tests. Link existence alone does not prove every assertion in a Python test is equivalent.

| Status | IDs | Meaning |
| --- | ---: | --- |
| mapped | 367 | Automated regression asserts the original behavior or its native equivalent. |
| intentional_difference | 35 | Tested policy deliberately replaces the original behavior; rationale is recorded per ID. |
| obsolete | 57 | Python implementation retired; no claim of automated native equivalence. |
| partial | 0 | Some parser/helper evidence exists; original behavior is not fully established. |
| manual | 0 | Manual evidence only; automated regression remains outstanding. |
| not_mapped | 0 | No usable regression link yet. |

**0 IDs remain incomplete for automated parity.** Parser fixture counts are not a substitute for safe transaction, timeout, identity, and recovery tests. Hardware-free I/O tests do not claim physical hardware validation. Only the attached Razer hardware was available for hardware smoke checks; GameSir/WGI and other vendors use simulated/pure report evidence.

Inputs: `docs/coverage_mapping_core.json`, `docs/coverage_mapping_provider_audio_mouse.json`, `docs/coverage_mapping_provider_controllers.json`, `docs/coverage_mapping_provider_family.json`, `docs/coverage_mapping_provider_logitech.json`, `docs/coverage_mapping_provider_misc.json`, `docs/coverage_mapping_providers.json`, `docs/coverage_mapping_storage.json`, `docs/coverage_mapping_ui.json`, `docs/coverage_mapping_windows.json`.

Run `python tools/merge-coverage.py` to regenerate, `python tools/merge-coverage.py --check` to validate generated evidence, and add `--require-complete` to enforce no partial/manual/unmapped IDs. The tool uses Python's standard library and does not import the original app or query hardware.

## Incomplete IDs

None. Retired implementation details and intentional differences remain identified separately above.
