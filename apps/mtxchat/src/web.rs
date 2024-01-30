use serde::{Deserialize, Serialize};
use ureq::serde_json::{Map, Value};
use ureq::{Agent, ErrorKind};
use url::Url;

use crate::Msg;

const ACCEPT: &str = "Accept";
const ACCEPT_JSON: &str = "application/json";
const AUTHORIZATION: &str = "Authorization";
const BEARER: &str = "Bearer ";

pub const MTX_LOGIN_PASSWORD: &str = "m.login.password";
const MTX_ID_USER: &str = "m.id.user";

pub fn get_username(user: &str) -> String {
    let i = match user.find('@') {
        Some(index) => index + 1,
        None => 0,
    };
    let j = match user.find(':') {
        Some(index) => index,
        None => user.len(),
    };
    (&user[i..j]).to_string()
}

fn serialize<T: ?Sized + Serialize>(object: &T) -> Option<String> {
    match ureq::serde_json::to_string(&object) {
        Ok(value) => Some(value),
        Err(e) => {
            log::info!("ERROR in serialize: {:?}", e);
            None
        }
    }
}

pub fn handle_response(maybe_response: Result<ureq::Response, ureq::Error>) -> Option<Value> {
    match maybe_response {
        Ok(response) => {
            if let Ok(body) = response.into_json() {
                Some(body)
            } else {
                log::info!("Error: could not convert response into JSON");
                None
            }
        }
        Err(ureq::Error::Status(code, response)) => {
            // the server returned an unexpected status code (such as 400, 500 etc)
            let err_body = response.into_string().unwrap();
            log::info!("ERROR code {} err_body = {}", code, err_body);
            None
        }
        Err(ureq::Error::Transport(kind)) => {
            match kind.kind() {
                ErrorKind::ConnectionFailed => log::warn!("TLS failure"),
                _ => log::info!("Transport error: {:?}", kind),
            };
            None
        }
    }
}

pub fn get_json(url: &Url, agent: &mut Agent) -> Result<ureq::Response, ureq::Error> {
    agent.get(&url.as_str()).set(ACCEPT, ACCEPT_JSON).call()
}

pub fn get_json_auth(url: &Url, token: &str, agent: &mut Agent) -> Result<ureq::Response, ureq::Error> {
    let mut authorization = String::from(BEARER);
    authorization.push_str(token);
    agent.get(&url.as_str()).set(ACCEPT, ACCEPT_JSON).set(AUTHORIZATION, &authorization).call()
}

pub fn post_string(url: &Url, request_body: &str, agent: &mut Agent) -> Result<ureq::Response, ureq::Error> {
    agent.post(&url.as_str()).set(ACCEPT, ACCEPT_JSON).send_string(request_body)
}

pub fn post_string_auth(
    url: &Url,
    request_body: &str,
    token: &str,
    agent: &mut Agent,
) -> Result<ureq::Response, ureq::Error> {
    let mut authorization = String::from(BEARER);
    authorization.push_str(token);
    agent
        .post(&url.as_str())
        .set(ACCEPT, ACCEPT_JSON)
        .set(AUTHORIZATION, &authorization)
        .send_string(request_body)
}

pub fn put_string_auth(
    url: &Url,
    request_body: &str,
    token: &str,
    agent: &mut Agent,
) -> Result<ureq::Response, ureq::Error> {
    let mut authorization = String::from(BEARER);
    authorization.push_str(token);
    agent
        .put(&url.as_str())
        .set(ACCEPT, ACCEPT_JSON)
        .set(AUTHORIZATION, &authorization)
        .send_string(request_body)
}

// --------------------------------

pub fn whoami(url: &mut Url, token: &str, agent: &mut Agent) -> Option<String> {
    url.set_path("_matrix/client/r0/account/whoami");
    if let Some(value) = handle_response(get_json_auth(&url, token, agent)) {
        if let Value::Object(body) = value {
            if let Some(Value::String(device_id)) = body.get("device_id") {
                log::info!("device_id = {}", device_id);
            }
            if let Some(Value::String(user_id)) = body.get("user_id") {
                log::info!("user_id = {}", user_id);
                return Some(user_id.to_string());
            }
        }
    }
    None
}

pub fn get_login_type(url: &mut Url, agent: &mut Agent) -> bool {
    url.set_path("_matrix/client/r0/login");
    let mut found = false;
    if let Some(value) = handle_response(get_json(&url, agent)) {
        if let Value::Object(body) = value {
            if let Some(Value::Array(flows)) = body.get("flows") {
                for flow in flows.iter() {
                    if let Some(Value::String(login_type)) = flow.get("type") {
                        if login_type.eq(MTX_LOGIN_PASSWORD) {
                            found = true;
                            break;
                        }
                    }
                }
            }
        }
    }
    found
}

#[derive(Serialize, Deserialize)]
struct AuthIdentifier {
    #[serde(rename = "type")]
    type_: String,
    user: String,
}

#[derive(Serialize, Deserialize)]
struct AuthRequest {
    #[serde(rename = "type")]
    type_: String,
    identifier: AuthIdentifier,
    password: String,
}

impl AuthRequest {
    pub fn new(user: &str, password: &str) -> Self {
        let identifier = AuthIdentifier { type_: MTX_ID_USER.to_string(), user: user.to_string() };
        AuthRequest { type_: MTX_LOGIN_PASSWORD.to_string(), identifier, password: password.to_string() }
    }
}

// fn authenticate_user() -> Result<String, ureq::Error> {
pub fn authenticate_user(url: &mut Url, user: &str, password: &str, agent: &mut Agent) -> Option<String> {
    let mut maybe_token: Option<String> = None;
    url.set_path("_matrix/client/r0/login");
    let auth_request = AuthRequest::new(user, password);
    if let Some(request_body) = serialize(&auth_request) {
        if let Some(value) = handle_response(post_string(&url, &request_body, agent)) {
            if let Value::Object(body) = value {
                if let Some(Value::String(access_token)) = body.get("access_token") {
                    maybe_token = Some(access_token.to_string())
                }
            }
        }
    }
    maybe_token
}

pub fn get_room_id(url: &mut Url, room_server: &str, token: &str, agent: &mut Agent) -> Option<String> {
    let mut path = String::from("_matrix/client/v3/directory/room/");
    path.push_str(&room_server);
    url.set_path(&path);
    log::info!("get_room_id = {}", url);
    if let Some(value) = handle_response(get_json_auth(&url, token, agent)) {
        if let Value::Object(body) = value {
            if let Some(Value::String(room_id)) = body.get("room_id") {
                Some(room_id.to_string())
            } else {
                log::info!("invalid response for get_room_id");
                None
            }
        } else {
            log::info!("invalid response for get_room_id");
            None
        }
    } else {
        log::info!("Error for get_room_id");
        None
    }
}

#[derive(Serialize, Deserialize)]
struct EventFilter {
    limit: i32,
    not_types: Vec<String>,
}

impl EventFilter {
    pub fn new(limit: i32) -> Self {
        let mut not_types: Vec<String> = Vec::new();
        not_types.push("*".to_string());
        EventFilter { limit, not_types }
    }
}

#[derive(Serialize, Deserialize)]
struct RoomEventFilter {
    limit: i32,
    types: Vec<String>,
    rooms: Vec<String>,
}

impl RoomEventFilter {
    pub fn new(limit: i32, room_id: &str, type_0: &str) -> Self {
        let mut types = Vec::new();
        types.push(type_0.to_string());
        let mut rooms: Vec<String> = Vec::new();
        rooms.push(room_id.to_string());
        RoomEventFilter { limit, types, rooms }
    }
}

#[derive(Serialize, Deserialize)]
struct RoomFilter {
    account_data: EventFilter, // Should be RoomEventFilter
    ephemeral: EventFilter,
    rooms: Vec<String>,
    state: EventFilter, // Should be StateFilter
    timeline: RoomEventFilter,
}

impl RoomFilter {
    pub fn new(room_id: &str) -> Self {
        let account_data = EventFilter::new(0);
        let ephemeral = EventFilter::new(0);
        let mut rooms: Vec<String> = Vec::new();
        rooms.push(room_id.to_string());
        let state = EventFilter::new(0);
        let timeline = RoomEventFilter::new(10, room_id, "m.room.message");
        RoomFilter { account_data, ephemeral, rooms, state, timeline }
    }
}

#[derive(Serialize, Deserialize)]
struct FilterRequest {
    account_data: EventFilter,
    event_fields: Vec<String>,
    presence: EventFilter,
    room: RoomFilter,
}

impl FilterRequest {
    pub fn new(room_id: &str) -> Self {
        let account_data = EventFilter::new(0);
        let mut event_fields: Vec<String> = Vec::new();
        event_fields.push("type".to_string());
        event_fields.push("sender".to_string());
        event_fields.push("content.body".to_string());
        event_fields.push("origin_server_ts".to_string());
        let presence = EventFilter::new(0);
        let room = RoomFilter::new(room_id);
        FilterRequest { account_data, event_fields, presence, room }
    }
}

pub fn get_filter(
    user: &str,
    url: &mut Url,
    room_id: &str,
    token: &str,
    agent: &mut Agent,
) -> Option<String> {
    let mut path = String::from("_matrix/client/v3/user/");
    path.push_str(&user);
    path.push_str("/filter");
    url.set_path(&path);
    log::info!("get_filter = {}", url.as_str());
    let filter_request = FilterRequest::new(room_id);
    if let Some(request_body) = serialize(&filter_request) {
        if let Some(value) = handle_response(post_string_auth(url, &request_body, token, agent)) {
            if let Value::Object(body) = value {
                if let Some(Value::String(filter_id)) = body.get("filter_id") {
                    log::info!("filter_id = {}", filter_id);
                    Some(filter_id.to_string())
                } else {
                    log::info!("invalid response for get_filter");
                    None
                }
            } else {
                log::info!("invalid response for get_filter");
                None
            }
        } else {
            log::info!("Error for get_filter");
            None
        }
    } else {
        log::info!("Error unable to serialize request for get_filter");
        None
    }
}

fn get_messages(body: Map<String, Value>, room_id: &str) -> Vec<Msg> {
    log::info!("heap usage: {}", crate::heap_usage());
    let mut msgs = Vec::<Msg>::new();
    if let Some(Value::Object(rooms)) = body.get("rooms") {
        if let Some(Value::Object(join)) = rooms.get("join") {
            if let Some(Value::Object(room)) = join.get(room_id) {
                if let Some(Value::Object(timeline)) = room.get("timeline") {
                    if let Some(Value::Array(events)) = timeline.get("events") {
                        for event in events.iter() {
                            log::trace!("{:?}", event);
                            if let Some(Value::String(type_)) = event.get("type") {
                                if type_.eq("m.room.message") {
                                    msgs.push(Msg {
                                        type_: type_.to_string(),
                                        body: event
                                            .get("content")
                                            .map(|c| c.get("body").map(|b| b.to_string()))
                                            .flatten(),
                                        sender: event.get("sender").map(|s| s.to_string()),
                                        ts: event.get("origin_server_ts").map(|t| t.as_u64()).flatten(),
                                    });
                                }
                            }
                        } // event
                    }
                }
            }
        }
    }
    msgs
}

pub fn client_sync(
    url: &mut Url,
    filter: &str,
    since: Option<&str>,
    timeout: i32,
    room_id: &str,
    token: &str,
    agent: &mut Agent,
) -> Option<(String, Vec<Msg>)> {
    log::info!("heap usage: {}", crate::heap_usage());
    url.set_path("_matrix/client/r0/sync");
    url.query_pairs_mut().append_pair("filter", &filter);
    url.query_pairs_mut().append_pair("timeout", &timeout.to_string());
    if let Some(since) = since {
        url.query_pairs_mut().append_pair("since", since);
    }
    log::info!("client_sync = {}", url.as_str());
    if let Some(value) = handle_response(get_json_auth(&url, token, agent)) {
        if let Value::Object(body) = value {
            if let Some(Value::String(next_batch)) = body.get("next_batch") {
                Some((next_batch.to_string(), get_messages(body, room_id)))
            } else {
                log::info!("invalid response for client_sync");
                None
            }
        } else {
            log::info!("Error for client_sync: deserialization");
            None
        }
    } else {
        None
    }
}

#[derive(Serialize, Deserialize)]
struct MessageRequest {
    msgtype: String,
    body: String,
}

impl MessageRequest {
    pub fn new(text: &str) -> Self {
        let msgtype = "m.text".to_string();
        let body = text.to_string();
        MessageRequest { msgtype, body }
    }
}

pub fn send_message(
    url: &mut Url,
    room_id: &str,
    text: &str,
    txn_id: &str,
    token: &str,
    agent: &mut Agent,
) -> bool {
    log::info!("heap usage: {}", crate::heap_usage());
    let mut path = String::from("_matrix/client/r0/rooms/");
    path.push_str(&room_id);
    path.push_str("/send/m.room.message/");
    path.push_str(&txn_id);
    url.set_path(&path);
    log::info!("send_message = {}", url);
    let message_request = MessageRequest::new(text);
    if let Some(request_body) = serialize(&message_request) {
        if let Some(value) = handle_response(put_string_auth(url, &request_body, token, agent)) {
            if let Value::Object(_body) = value {
                true
            } else {
                log::info!("invalid response for send_message");
                false
            }
        } else {
            log::info!("Error for send_message");
            false
        }
    } else {
        log::info!("Error unable to serialize request for send_message");
        false
    }
}
