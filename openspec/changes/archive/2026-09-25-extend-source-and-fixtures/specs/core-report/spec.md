# Spec Delta

## MODIFIED Requirements

### Requirement: JSON report
With `--json`, the tool SHALL print one JSON object with `version`, `sampling`
(`interval`, `count`), `system`, `overall` and a `sections` array. Each section object has
`id`, `title`, `equivalent` (the article command), `resource`, `status`, `summary`, `details`,
`findings` and `metrics`. `resource` SHALL be one of `cpu`, `memory`, `disk`, `network`,
`capacity`, `hardware`, `kernel` or `pressure`.

#### Scenario: JSON is parseable
- **WHEN** run with `--json`
- **THEN** stdout is a single valid JSON document containing every section

#### Scenario: Resource per section
- **WHEN** run with `--json`
- **THEN** the `disk` section has `"resource": "disk"` and the `cpu` section has `"resource": "cpu"`
