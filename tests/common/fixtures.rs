// Test data fixtures and factories
#![allow(dead_code)]

/// Creates a simple report request for testing middleware injection
///
/// This minimal payload contains only flat resources and events without nested relationships.
/// Use this for testing middleware and authentication flows without complex business logic.
pub fn create_simple_test_report_request() -> serde_json::Value {
    serde_json::json!({
        "resource_captures": [
            {
                "type": "Test API",
                "id": "api.example.com",
                "globally_unique": null,
                "first_seen_at": "2025-10-16T01:00:00Z",
                "last_seen_at": "2025-10-16T02:00:00Z",
                "attributes": null,
                "contains": null
            }
        ],
        "event_captures": []
    })
}

/// Creates a minimal but valid report request for testing report ingestion logic
///
/// The simplified payload includes:
/// - 2 resource captures (1 flat API resource, 1 nested Kubernetes structure)
/// - 2 event captures (1 API access event, 1 K8s service targeting event)
///
/// This tests the core business logic:
/// - Resource tree upsert with nested contains relationships
/// - Principal chain creation and linking
/// - Event relationship creation between principals and resources
/// - Timestamp handling (`first_seen_at``last_seen_at`at)
pub fn create_test_report_request() -> serde_json::Value {
    serde_json::json!({
        "resource_captures": [
            {
                "type": "Test API",
                "id": "api.example.com",
                "globally_unique": null,
                "first_seen_at": "2025-10-16T01:00:00Z",
                "last_seen_at": "2025-10-16T02:00:00Z",
                "attributes": null,
                "contains": null
            },
            {
                "type": "Kubernetes Cluster",
                "id": "test-cluster-123",
                "globally_unique": null,
                "first_seen_at": "2025-10-16T01:00:00Z",
                "last_seen_at": "2025-10-16T02:00:00Z",
                "attributes": null,
                "contains": [
                    {
                        "type": "Namespace",
                        "id": "prod",
                        "globally_unique": null,
                        "first_seen_at": "2025-10-16T01:00:00Z",
                        "last_seen_at": "2025-10-16T02:00:00Z",
                        "attributes": null,
                        "contains": [
                            {
                                "type": "Service",
                                "id": "api-service",
                                "globally_unique": null,
                                "first_seen_at": "2025-10-16T01:00:00Z",
                                "last_seen_at": "2025-10-16T02:00:00Z",
                                "attributes": null,
                                "contains": null
                            }
                        ]
                    }
                ]
            }
        ],
        "event_captures": [
            {
                "principals": [
                    {
                        "id": [
                            ["Kubernetes Cluster", "test-cluster-123"],
                            ["Namespace", "prod"],
                            ["Container", "test-container"]
                        ],
                        "event": null
                    }
                ],
                "resources": [
                    [
                        ["Test API", "api.example.com"]
                    ]
                ],
                "events": [
                    {
                        "type": "Accessed",
                        "first_seen_at": "2025-10-16T01:30:00Z",
                        "last_seen_at": "2025-10-16T01:45:00Z"
                    }
                ]
            },
            {
                "principals": [
                    {
                        "id": [
                            ["Kubernetes Cluster", "test-cluster-123"],
                            ["Namespace", "prod"],
                            ["Service", "frontend"]
                        ],
                        "event": null
                    }
                ],
                "resources": [
                    [
                        ["Kubernetes Cluster", "test-cluster-123"],
                        ["Namespace", "prod"],
                        ["Service", "api-service"]
                    ]
                ],
                "events": [
                    {
                        "type": "Targeted",
                        "first_seen_at": "2025-10-16T01:00:00Z",
                        "last_seen_at": "2025-10-16T02:00:00Z"
                    }
                ]
            }
        ]
    })
}
