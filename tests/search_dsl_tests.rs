use eventlake::search::{
    RawLogSearchRequest, SearchFilter, SearchOperator, SearchRequest,
    validate_raw_log_search_request, validate_search_request,
};
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

#[test]
fn raw_log_search_requires_chain_and_accepts_positional_topics() {
    let topic0 = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
    let request = RawLogSearchRequest {
        page: Some(1),
        limit: Some(50),
        filters: vec![
            SearchFilter {
                field: "chain_id".to_owned(),
                operator: SearchOperator::Eq,
                value: json!(1),
            },
            SearchFilter {
                field: "block_number".to_owned(),
                operator: SearchOperator::Gte,
                value: json!(22_000_000),
            },
            SearchFilter {
                field: "topic0".to_owned(),
                operator: SearchOperator::Eq,
                value: json!(topic0),
            },
        ],
        sort: None,
    };

    validate_raw_log_search_request(&request).expect("valid raw-log search");

    let missing_chain = RawLogSearchRequest {
        filters: request.filters.into_iter().skip(1).collect(),
        page: None,
        limit: None,
        sort: None,
    };
    assert!(validate_raw_log_search_request(&missing_chain).is_err());
}
