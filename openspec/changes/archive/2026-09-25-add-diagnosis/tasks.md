# Tasks

## 1. Finding resources

- [x] 1.1 `Finding.resource` plus `note_on`/`warn_on`/`crit_on`/`threshold_on` and `Section::resource_of`; verify `cargo test`
- [x] 1.2 Tag cross-resource findings in pressure, cpu (iowait), cgroup (memory events) and kernel-log (by event); verify the existing tests of those checks

## 2. Diagnosis

- [x] 2.1 `report::diagnosis::diagnose` with ranking, labels and evidence shortening; verify the tests in `report/diagnosis.rs` (one per spec scenario)
- [x] 2.2 Text rendering under OVERALL and the JSON `diagnosis` field; verify `plain_text_layout` and the `json` tests (including `diagnosis_is_null_when_all_ok`)
- [x] 2.3 Pluralize "warnings" in the OVERALL counts; verify the legacy fixture report

## 3. Integration

- [x] 3.1 Container runs: CPU quota load → `CPU quota throttling (cgroup)`; 4 parallel direct-I/O writers → `disk I/O`
