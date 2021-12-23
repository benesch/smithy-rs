/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_sdk_dynamodb::model::AttributeValue;
use aws_sdk_dynamodb::{Client, Config};
use aws_smithy_client::test_connection::{capture_request, TestConnection};
use aws_smithy_http::body::SdkBody;
use aws_smithy_protocol_test::{assert_ok, validate_body, MediaType};
use aws_types::region::Region;
use aws_types::Credentials;
use std::collections::HashMap;
use std::iter::FromIterator;
use tokio_stream::StreamExt;

fn stub_config() -> Config {
    Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new("akid", "secret", None, None, "test"))
        .build()
}

/// Validate that arguments are passed on to the paginator
#[tokio::test]
async fn paginators_pass_args() {
    let (conn, request) = capture_request(None);
    let client = Client::from_conf_conn(stub_config(), conn);
    let mut paginator = client
        .scan()
        .table_name("test-table")
        .paginate()
        .page_size(32)
        .send()
        .await;
    let _ = paginator.next().await;
    let request = request.expect_request();
    let body = request.body().bytes().expect("data is loaded");
    assert_ok(validate_body(
        body,
        r#"{"TableName":"test-table","Limit":32}"#,
        MediaType::Json,
    ));
}

fn mk_reqest(body: &'static str) -> http::Request<SdkBody> {
    http::Request::builder()
        .uri("https://dynamodb.us-east-1.amazonaws.com/")
        .body(SdkBody::from(body))
        .unwrap()
}

fn mk_response(body: &'static str) -> http::Response<SdkBody> {
    http::Response::builder().body(SdkBody::from(body)).unwrap()
}

#[tokio::test]
async fn paginators_loop_until_completion() {
    let conn = TestConnection::new(vec![
        (
            mk_reqest(r#"{"TableName":"test-table","Limit":32}"#),
            mk_response(
                r#"{
                            "Count": 1,
                            "Items": [{
                                "PostedBy": {
                                    "S": "joe@example.com"
                                }
                            }],
                            "LastEvaluatedKey": {
                                "PostedBy": { "S": "joe@example.com" }
                            }
                        }"#,
            ),
        ),
        (
            mk_reqest(
                r#"{"TableName":"test-table","Limit":32,"ExclusiveStartKey":{"PostedBy":{"S":"joe@example.com"}}}"#,
            ),
            mk_response(
                r#"{
                            "Count": 1,
                            "Items": [{
                                "PostedBy": {
                                    "S": "jack@example.com"
                                }
                            }]
                        }"#,
            ),
        ),
    ]);
    let client = Client::from_conf_conn(stub_config(), conn.clone());
    let mut paginator = client
        .scan()
        .table_name("test-table")
        .paginate()
        .page_size(32)
        .send()
        .await;
    assert_eq!(conn.requests().len(), 0);
    let first_page = paginator
        .try_next()
        .await
        .expect("success")
        .expect("page exists");
    assert_eq!(
        first_page.items.unwrap_or_default(),
        vec![HashMap::from_iter([(
            "PostedBy".to_string(),
            AttributeValue::S("joe@example.com".to_string())
        )])]
    );
    assert_eq!(conn.requests().len(), 1);
    let second_page = paginator
        .try_next()
        .await
        .expect("success")
        .expect("page exists");
    assert_eq!(
        second_page.items.unwrap_or_default(),
        vec![HashMap::from_iter([(
            "PostedBy".to_string(),
            AttributeValue::S("jack@example.com".to_string())
        )])]
    );
    assert_eq!(conn.requests().len(), 2);
    assert!(
        paginator.next().await.is_none(),
        "no more pages should exist"
    );
    // we shouldn't make another request, we know we're at the end
    assert_eq!(conn.requests().len(), 2);
    conn.assert_requests_match(&[]);
}

#[tokio::test]
async fn paginators_handle_errors() {
    // LastEvaluatedKey is set but there is only one response in the test connection
    let conn = TestConnection::new(vec![(
        mk_reqest(r#"{"TableName":"test-table","Limit":32}"#),
        mk_response(
            r#"{
                            "Count": 1,
                            "Items": [{
                                "PostedBy": {
                                    "S": "joe@example.com"
                                }
                            }],
                            "LastEvaluatedKey": {
                                "PostedBy": { "S": "joe@example.com" }
                            }
                        }"#,
        ),
    )]);
    let client = Client::from_conf_conn(stub_config(), conn.clone());
    let mut rows = client
        .scan()
        .table_name("test-table")
        .paginate()
        .page_size(32)
        .items()
        .send()
        .await;
    assert_eq!(
        rows.try_next()
            .await
            .expect("no error")
            .expect("not EOS")
            .get("PostedBy"),
        Some(&AttributeValue::S("joe@example.com".to_string()))
    );
    rows.try_next().await.expect_err("failure");
    assert_eq!(rows.try_next().await.expect("ok"), None);
}
