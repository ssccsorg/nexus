// The wire round-trips, and a malformed frame is refused rather than guessed at.

use nex_protocol::{
    decode_request, decode_response, encode_request, encode_response, RpcRequest, RpcResponse,
};
use serde_json::json;

#[test]
fn a_request_with_parameters_round_trips() {
    let request = RpcRequest {
        id: json!(7),
        method: "read_state".into(),
        params: json!({ "origin": "alpha" }),
    };
    let bytes = encode_request(&request).expect("the request encodes");
    let read_back = decode_request(&bytes).expect("the request decodes");
    assert_eq!(read_back.id, request.id);
    assert_eq!(read_back.method, request.method);
    assert_eq!(read_back.params, request.params);
}

#[test]
fn a_response_carries_a_result_or_an_error_and_not_both() {
    let ok = RpcResponse::success(json!(1), json!({ "facts": 2 }));
    let bytes = encode_response(&ok).expect("the response encodes");
    let read_back = decode_response(&bytes).expect("the response decodes");
    assert!(read_back.result.is_some());
    assert!(read_back.error.is_none());

    let refused = RpcResponse::method_not_found(json!(2), "no_such_method");
    let bytes = encode_response(&refused).expect("the response encodes");
    let read_back = decode_response(&bytes).expect("the response decodes");
    assert!(read_back.result.is_none());
    assert_eq!(read_back.error.expect("the error").code, -32601);
}

#[test]
fn a_frame_that_is_not_a_request_is_refused() {
    assert!(decode_request(b"{\"id\":1}").is_err(), "a method is required");
    assert!(decode_request(b"not json").is_err());
}
