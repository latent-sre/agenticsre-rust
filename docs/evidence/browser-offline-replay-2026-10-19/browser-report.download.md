# Evidence handoff

Historical evidence only. Source text and proposed knowledge are inert data below.

## Title

    "Historical handoff <script>window.investigationExecuted=1</script>"

## Generated at

    "2026-10-20T03:50:47.809687795Z"

## Tool

    {
      "id": "save",
      "version": "0.1.0"
    }

## Report state

    "complete"

## Scope

    "historical_evidence"

## Execution profile

    "linux-offline-v1"

## Evidence sources

    [
      {
        "bundle": {
          "digest": "sha256:934501b62b26d4cde3bf582b7c5a0c748ca79435dfd77e17fb0561bb87715299",
          "id": "run-9-18dd60cacef0724e-1"
        },
        "evidence_state": "sourced",
        "observed_at": "2026-10-11T04:57:30.934817013Z",
        "reference": "evidence-bundle:run-9-18dd60cacef0724e-1@sha256:934501b62b26d4cde3bf582b7c5a0c748ca79435dfd77e17fb0561bb87715299",
        "untrusted": true
      },
      {
        "bundle": {
          "digest": "sha256:e1a567146071ece2ed5e32294c81da4e647ce9acc9115e2bbd3719f11fb62a1a",
          "id": "run-16-18dd60cad194352b-1"
        },
        "evidence_state": "sourced",
        "observed_at": "2026-10-11T04:57:30.979123506Z",
        "reference": "evidence-bundle:run-16-18dd60cad194352b-1@sha256:e1a567146071ece2ed5e32294c81da4e647ce9acc9115e2bbd3719f11fb62a1a",
        "untrusted": true
      }
    ]

## Historical observations

    [
      {
        "bundle": {
          "digest": "sha256:934501b62b26d4cde3bf582b7c5a0c748ca79435dfd77e17fb0561bb87715299",
          "id": "run-9-18dd60cacef0724e-1"
        },
        "capture": {
          "expected_steps": 2,
          "retained_receipts": 2,
          "state": "complete"
        },
        "checks": [
          {
            "definition": {
              "dialect": "promql",
              "id": "prometheus.up-by-job",
              "sha256": "e42bc20e72b99554cc61b2c09d06b2dd3ad028a1455aa835517f2dd956c1363f",
              "version": 1
            },
            "details": [
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "a",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899200000000000",
                "type": "metric_sample",
                "value": 1
              },
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "a",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899201000000000",
                "type": "metric_sample",
                "value": 1
              },
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "removed",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899200000000000",
                "type": "metric_sample",
                "value": 0
              },
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "removed",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899201000000000",
                "type": "metric_sample",
                "value": 1
              }
            ],
            "id": "metrics",
            "kind": "prometheus",
            "omitted_details": 0,
            "reasons": [],
            "state": "complete",
            "summary": {
              "finite": 4,
              "missing": 0,
              "nonfinite": 0,
              "samples": 4,
              "sampling_gaps": 0,
              "series": 2
            },
            "unit": "dimensionless"
          },
          {
            "definition": {
              "dialect": "logql",
              "id": "loki.logs-by-job",
              "sha256": "d4661dbe0efc261ccb5eb340cbcbceb90568fa2e0973b6518cfa4fd796eb6019",
              "version": 1
            },
            "details": [
              {
                "count": 2,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "duplicate",
                "timestamp_ns": "1790899200000000123",
                "type": "log_entry"
              },
              {
                "count": 1,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "removed",
                "timestamp_ns": "1790899200000000123",
                "type": "log_entry"
              }
            ],
            "id": "logs",
            "kind": "loki",
            "omitted_details": 0,
            "reasons": [],
            "state": "complete",
            "summary": {
              "duplicate_entries": 1,
              "entries": 3,
              "label_sets": 1,
              "unique_entries": 2
            },
            "unit": "entries"
          }
        ],
        "finalized_at": "2026-10-11T04:57:30.97401695Z",
        "manifest_state": "sealed",
        "provenance": {
          "context": {
            "record": {
              "bindings": {
                "environment": "dev",
                "organization": "7",
                "target": "fixture"
              },
              "deployment": "blue",
              "environment": "dev",
              "last_reviewed": "2026-10-11T04:57:30.932728+00:00",
              "service": "fixture-web",
              "state": "active",
              "team": "platform"
            },
            "resolution": {
              "age_seconds": 0,
              "max_age_days": 30,
              "state": "resolved",
              "usable": true
            },
            "source": {
              "evidence_state": "sourced",
              "id": "comparison",
              "kind": "fixture-export",
              "observed_at": "2026-10-11T04:57:30.93913915Z",
              "revision": "1",
              "sha256": "sha256:4e9f959c5427e061af22d47f845e048feab09b854661aed029b961b2abec9c9b"
            }
          },
          "grafana_ca": {
            "kind": "custom",
            "sha256": "sha256:b47321c8115362e4d8311a999ff98117a71a2c1173885647394fa9b1356b892e"
          },
          "grafana_config_sha256": "sha256:280cbe6b1ba60b867502f5cded9dee12cac28ca935e08761c42f73ce09eb0f3f"
        },
        "redaction": {
          "applied": false,
          "replacements": 0
        },
        "selection": {
          "deployment": null,
          "environment": "dev",
          "service": "fixture-web",
          "team": null
        },
        "source_limitations": [],
        "started_at": "2026-10-11T04:57:30.934817013Z",
        "state": "complete",
        "window": {
          "from": "2026-10-02T00:00:00Z",
          "to": "2026-10-02T00:05:00Z"
        }
      },
      {
        "bundle": {
          "digest": "sha256:e1a567146071ece2ed5e32294c81da4e647ce9acc9115e2bbd3719f11fb62a1a",
          "id": "run-16-18dd60cad194352b-1"
        },
        "capture": {
          "expected_steps": 2,
          "retained_receipts": 2,
          "state": "complete"
        },
        "checks": [
          {
            "definition": {
              "dialect": "promql",
              "id": "prometheus.up-by-job",
              "sha256": "e42bc20e72b99554cc61b2c09d06b2dd3ad028a1455aa835517f2dd956c1363f",
              "version": 1
            },
            "details": [
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "a",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899200000000000",
                "type": "metric_sample",
                "value": 1
              },
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "a",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899201000000000",
                "type": "metric_sample",
                "value": 0
              },
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "added",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899200000000000",
                "type": "metric_sample",
                "value": 0
              },
              {
                "nonfinite": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "added",
                    "job": "fixture"
                  }
                },
                "state": "finite",
                "timestamp_ns": "1790899201000000000",
                "type": "metric_sample",
                "value": 1
              }
            ],
            "id": "metrics",
            "kind": "prometheus",
            "omitted_details": 0,
            "reasons": [],
            "state": "complete",
            "summary": {
              "finite": 4,
              "missing": 0,
              "nonfinite": 0,
              "samples": 4,
              "sampling_gaps": 0,
              "series": 2
            },
            "unit": "dimensionless"
          },
          {
            "definition": {
              "dialect": "logql",
              "id": "loki.logs-by-job",
              "sha256": "d4661dbe0efc261ccb5eb340cbcbceb90568fa2e0973b6518cfa4fd796eb6019",
              "version": 1
            },
            "details": [
              {
                "count": 1,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "added",
                "timestamp_ns": "1790899200000000123",
                "type": "log_entry"
              },
              {
                "count": 1,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "duplicate",
                "timestamp_ns": "1790899200000000123",
                "type": "log_entry"
              }
            ],
            "id": "logs",
            "kind": "loki",
            "omitted_details": 0,
            "reasons": [],
            "state": "complete",
            "summary": {
              "duplicate_entries": 0,
              "entries": 2,
              "label_sets": 1,
              "unique_entries": 2
            },
            "unit": "entries"
          }
        ],
        "finalized_at": "2026-10-11T04:57:31.017403549Z",
        "manifest_state": "sealed",
        "provenance": {
          "context": {
            "record": {
              "bindings": {
                "environment": "dev",
                "organization": "7",
                "target": "fixture"
              },
              "deployment": "blue",
              "environment": "dev",
              "last_reviewed": "2026-10-11T04:57:30.932728+00:00",
              "service": "fixture-web",
              "state": "active",
              "team": "platform"
            },
            "resolution": {
              "age_seconds": 0,
              "max_age_days": 30,
              "state": "resolved",
              "usable": true
            },
            "source": {
              "evidence_state": "sourced",
              "id": "comparison",
              "kind": "fixture-export",
              "observed_at": "2026-10-11T04:57:30.983081172Z",
              "revision": "1",
              "sha256": "sha256:4e9f959c5427e061af22d47f845e048feab09b854661aed029b961b2abec9c9b"
            }
          },
          "grafana_ca": {
            "kind": "custom",
            "sha256": "sha256:b47321c8115362e4d8311a999ff98117a71a2c1173885647394fa9b1356b892e"
          },
          "grafana_config_sha256": "sha256:280cbe6b1ba60b867502f5cded9dee12cac28ca935e08761c42f73ce09eb0f3f"
        },
        "redaction": {
          "applied": false,
          "replacements": 0
        },
        "selection": {
          "deployment": null,
          "environment": "dev",
          "service": "fixture-web",
          "team": null
        },
        "source_limitations": [],
        "started_at": "2026-10-11T04:57:30.979123506Z",
        "state": "complete",
        "window": {
          "from": "2026-10-02T00:00:00Z",
          "to": "2026-10-02T00:05:00Z"
        }
      }
    ]

## Comparison findings

    [
      {
        "after": {
          "digest": "sha256:e1a567146071ece2ed5e32294c81da4e647ce9acc9115e2bbd3719f11fb62a1a",
          "id": "run-16-18dd60cad194352b-1"
        },
        "before": {
          "digest": "sha256:934501b62b26d4cde3bf582b7c5a0c748ca79435dfd77e17fb0561bb87715299",
          "id": "run-9-18dd60cacef0724e-1"
        },
        "checks": [
          {
            "definition": {
              "dialect": "promql",
              "id": "prometheus.up-by-job",
              "sha256": "e42bc20e72b99554cc61b2c09d06b2dd3ad028a1455aa835517f2dd956c1363f",
              "version": 1
            },
            "details": [
              {
                "after": 0,
                "after_state": "finite",
                "after_timestamp_ns": "1790899201000000000",
                "before": 1,
                "before_state": "finite",
                "before_timestamp_ns": "1790899201000000000",
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "a",
                    "job": "fixture"
                  }
                },
                "timestamp_ns": "1790899201000000000",
                "type": "sample_changed"
              },
              {
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "added",
                    "job": "fixture"
                  }
                },
                "type": "series_added"
              },
              {
                "after": 0,
                "after_state": "finite",
                "after_timestamp_ns": "1790899200000000000",
                "before": null,
                "before_state": "absent",
                "before_timestamp_ns": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "added",
                    "job": "fixture"
                  }
                },
                "timestamp_ns": "1790899200000000000",
                "type": "sample_added"
              },
              {
                "after": 1,
                "after_state": "finite",
                "after_timestamp_ns": "1790899201000000000",
                "before": null,
                "before_state": "absent",
                "before_timestamp_ns": null,
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "added",
                    "job": "fixture"
                  }
                },
                "timestamp_ns": "1790899201000000000",
                "type": "sample_added"
              },
              {
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "removed",
                    "job": "fixture"
                  }
                },
                "type": "series_removed"
              },
              {
                "after": null,
                "after_state": "absent",
                "after_timestamp_ns": null,
                "before": 0,
                "before_state": "finite",
                "before_timestamp_ns": "1790899200000000000",
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "removed",
                    "job": "fixture"
                  }
                },
                "timestamp_ns": "1790899200000000000",
                "type": "sample_removed"
              },
              {
                "after": null,
                "after_state": "absent",
                "after_timestamp_ns": null,
                "before": 1,
                "before_state": "finite",
                "before_timestamp_ns": "1790899201000000000",
                "series": {
                  "field": "up",
                  "labels": {
                    "instance": "removed",
                    "job": "fixture"
                  }
                },
                "timestamp_ns": "1790899201000000000",
                "type": "sample_removed"
              }
            ],
            "id": "metrics",
            "kind": "prometheus",
            "omitted_details": 0,
            "reasons": [],
            "state": "complete",
            "summary": {
              "after": {
                "missing": 0,
                "nonfinite": 0,
                "samples": 4,
                "sampling_gaps": 0,
                "series": 2
              },
              "before": {
                "missing": 0,
                "nonfinite": 0,
                "samples": 4,
                "sampling_gaps": 0,
                "series": 2
              },
              "samples_added": 2,
              "samples_changed": 1,
              "samples_missing": 0,
              "samples_removed": 2,
              "samples_unchanged": 1,
              "sampling": {
                "after_declared_intervals_ms": [
                  1000
                ],
                "before_declared_intervals_ms": [
                  1000
                ],
                "request_interval_ms": 1000
              },
              "series_added": 1,
              "series_removed": 1
            },
            "unit": "dimensionless"
          },
          {
            "definition": {
              "dialect": "logql",
              "id": "loki.logs-by-job",
              "sha256": "d4661dbe0efc261ccb5eb340cbcbceb90568fa2e0973b6518cfa4fd796eb6019",
              "version": 1
            },
            "details": [
              {
                "after_count": 1,
                "after_timestamp_ns": "1790899200000000123",
                "before_count": 0,
                "before_timestamp_ns": null,
                "count": 1,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "added",
                "timestamp_ns": "1790899200000000123",
                "type": "entry_added"
              },
              {
                "after_count": 1,
                "after_timestamp_ns": "1790899200000000123",
                "before_count": 2,
                "before_timestamp_ns": "1790899200000000123",
                "count": 1,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "duplicate",
                "timestamp_ns": "1790899200000000123",
                "type": "entry_removed"
              },
              {
                "after_count": 0,
                "after_timestamp_ns": null,
                "before_count": 1,
                "before_timestamp_ns": "1790899200000000123",
                "count": 1,
                "label_types": {},
                "labels": {
                  "job": "fixture"
                },
                "line": "removed",
                "timestamp_ns": "1790899200000000123",
                "type": "entry_removed"
              }
            ],
            "id": "logs",
            "kind": "loki",
            "omitted_details": 0,
            "reasons": [],
            "state": "complete",
            "summary": {
              "after": {
                "entries": 2,
                "label_sets": 1
              },
              "before": {
                "entries": 3,
                "label_sets": 1
              },
              "entries_added": 1,
              "entries_removed": 2,
              "entries_unchanged": 1
            },
            "unit": "entries"
          }
        ],
        "compatibility": {
          "reasons": [],
          "state": "compatible"
        },
        "finding": "disagreement",
        "provenance_changes": [
          "/provenance/context/source/observed_at"
        ],
        "source_limitations": [],
        "state": "complete"
      }
    ]

## Limitations

    [
      "Historical observations are not current service health. Missing samples are not zero; absent logs are not an all-clear. Source integrity does not establish truth, authorship, causation or complete telemetry coverage.",
      "Only installed saved-query frame profiles are analyzed. Details are bounded; omission counts, source redactions and incompatible checks remain explicit."
    ]

## Next questions

    [
      "Which separately requested live observations are needed to establish current conditions?",
      "What explains the differing retained observations? Check sampling, source identity and collection timing before proposing a cause."
    ]

## Pending knowledge proposals

    []

