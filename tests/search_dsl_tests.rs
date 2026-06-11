use eventlake::search::{SearchFilter, SearchOperator, SearchRequest, validate_search_request};
use serde_json::json;

#[test]
fn accepts_whitelisted_search_fields() {
    let request = SearchRequest {
        page: Some(1),
        limit: Some(50),
        filters: vec![
            SearchFilter {
                field: "event_name".to_owned(),
                operator: SearchOperator::Eq,
                value: json!("Transfer"),
            },
            SearchFilter {
                field: "field.value".to_owned(),
                operator: SearchOperator::Contains,
                value: json!("1000"),
            },
        ],
        sort: None,
    };

    validate_search_request(&request).expect("valid search request");
}

#[test]
fn rejects_unknown_search_fields() {
    let request = SearchRequest {
        page: Some(1),
        limit: Some(50),
        filters: vec![SearchFilter {
            field: "raw_sql".to_owned(),
            operator: SearchOperator::Eq,
            value: json!("1 = 1"),
        }],
        sort: None,
    };

    assert!(validate_search_request(&request).is_err());
}
