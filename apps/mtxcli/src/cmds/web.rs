use serde::{Serialize,Deserialize};
use ureq::serde_json::{Value, Map};
use ureq;

use crate::cmds::url;

const ACCEPT: &str = "Accept";
const ACCEPT_JSON: &str = "application/json";
const AUTHORIZATION: &str = "Authorization";
const BEARER: &str = "Bearer ";

pub const MTX_LOGIN_PASSWORD: &str = "m.login.password";
const MTX_ID_USER: &str = "m.id.user";

pub fn get_username(user: &str) -> String {
    let i = match user.find('@') {
        Some(index) => { index + 1 },
        None => { 0 },
    };
    let j = match user.find(':') {
        Some(index) => { index },
        None => { user.len() },
    };
    (&user[i..j]).to_string()
}

fn serialize<T: ?Sized + Serialize>(object: &T) -> Option<String> {
    match ureq::serde_json::to_string(&object) {
        Ok(value) => {
            Some(value)
        },
        Err(e) => {
            log::error!("ERROR in serialize: {:?}", e);
            None
        }
    }
}

pub fn handle_response(maybe_response: Result<ureq::Response,ureq::Error>) -> Option<Value> {
    match maybe_response {
        Ok(response) => {
            if let Ok(body) = response.into_json() {
                Some(body)
            } else {
                log::error!("Error: could not convert response into JSON");
                None
            }
        },
        Err(ureq::Error::Status(code, response)) => {
            /* the server returned an unexpected status
            code (such as 400, 500 etc) */
            let err_body = response.into_string().unwrap();
            log::error!("ERROR code {} err_body = {}", code, err_body);
            None
        }
        Err(e) => {
            log::error!("ERROR in handle_response: {:?}", e);
            None
        }
    }
}

pub fn get_json(url: &str) -> Result<ureq::Response, ureq::Error> {
    ureq::get(&url)
        .set(ACCEPT, ACCEPT_JSON)
        .call()
}

pub fn get_json_auth(url: &str, token:&str) -> Result<ureq::Response, ureq::Error> {
    let mut authorization = String::from(BEARER);
    authorization.push_str(token);
    ureq::get(&url)
        .set(ACCEPT, ACCEPT_JSON)
        .set(AUTHORIZATION, &authorization)
        .call()
}

pub fn post_string(url: &str, request_body: &str) -> Result<ureq::Response, ureq::Error> {
    ureq::post(&url)
        .set(ACCEPT, ACCEPT_JSON)
        .send_string(request_body)
}

pub fn post_string_auth(url: &str, request_body: &str, token: &str) -> Result<ureq::Response, ureq::Error> {
    let mut authorization = String::from(BEARER);
    authorization.push_str(token);
    ureq::post(&url)
        .set(ACCEPT, ACCEPT_JSON)
        .set(AUTHORIZATION, &authorization)
        .send_string(request_body)
}

pub fn put_string_auth(url: &str, request_body: &str, token: &str) -> Result<ureq::Response, ureq::Error> {
    let mut authorization = String::from(BEARER);
    authorization.push_str(token);
    ureq::put(&url)
        .set(ACCEPT, ACCEPT_JSON)
        .set(AUTHORIZATION, &authorization)
        .send_string(request_body)
}

// --------------------------------

pub fn whoami(server: &str, token: &str) -> bool {
    let mut url = String::from(server);
    url.push_str("/_matrix/client/r0/account/whoami");
    if let Some(value) = handle_response(get_json_auth(&url, token)) {
        if let Value::Object(body) = value {
            if let Some(Value::String(device_id)) = body.get("device_id") {
                log::debug!("device_id = {}", device_id);
            }
            if let Some(Value::String(user_id)) = body.get("user_id") {
                log::debug!("user_id = {}", user_id);
            }
        }
        true
    } else {
        false
    }
}

pub fn get_login_type(server: &str) -> bool {
    let mut url = String::from(server);
    url.push_str("/_matrix/client/r0/login");
    let mut found = false;
    if let Some(value) = handle_response(get_json(&url)) {
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
        let identifier = AuthIdentifier {
            type_: MTX_ID_USER.to_string(),
            user: user.to_string()
        };
        AuthRequest {
            type_: MTX_LOGIN_PASSWORD.to_string(),
            identifier: identifier,
            password: password.to_string()
        }
    }
}

// fn authenticate_user() -> Result<String, ureq::Error> {
pub fn authenticate_user(server: &str, user: &str, password: &str) -> Option<String> {
    let mut maybe_token: Option<String> = None;
    let mut url = String::from(server);
    url.push_str("/_matrix/client/r0/login");
    let auth_request = AuthRequest::new(user, password);
    if let Some(request_body) = serialize(&auth_request) {
        if let Some(value) = handle_response(post_string(&url, &request_body)) {
            if let Value::Object(body) = value {
                if let Some(Value::String(access_token)) = body.get("access_token") {
                    maybe_token = Some(access_token.to_string())
                }
            }
        }
    }
    maybe_token
}

pub fn get_room_id(server: &str, room_server: &str, token: &str) -> Option<String> {
    let room_encoded = url::encode(room_server);
    let mut url = String::from(server);
    url.push_str("/_matrix/client/v3/directory/room/");
    url.push_str(&room_encoded);
    log::debug!("get_room_id = {}", url);
    if let Some(value) = handle_response(get_json_auth(&url, token)) {
        if let Value::Object(body) = value {
            if let Some(Value::String(room_id)) = body.get("room_id") {
                Some(room_id.to_string())
            } else {
                log::error!("invalid response for get_room_id");
                None
            }
        } else {
            log::error!("invalid response for get_room_id");
            None
        }
    } else {
        log::error!("Error for get_room_id");
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
        EventFilter {
            limit,
            not_types,
        }
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
        RoomEventFilter {
            limit,
            types,
            rooms,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RoomFilter {
    account_data: EventFilter,  // Should be RoomEventFilter
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
        RoomFilter {
            account_data,
            ephemeral,
            rooms,
            state,
            timeline,
        }
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
        let presence = EventFilter::new(0);
        let room = RoomFilter::new(room_id);
        FilterRequest {
            account_data,
            event_fields,
            presence,
            room,
        }
    }
}

pub fn get_filter(user: &str, server: &str, room_id: &str, token: &str)
                  -> Option<String> {
    let user_encoded = url::encode(user);
    let mut url = String::from(server);
    url.push_str("/_matrix/client/v3/user/");
    url.push_str(&user_encoded);
    url.push_str("/filter");
    log::debug!("get_filter = {}", url);
    let filter_request = FilterRequest::new(room_id);
    if let Some(request_body) = serialize(&filter_request) {
        if let Some(value) = handle_response(post_string_auth(&url, &request_body, token)) {
            if let Value::Object(body) = value {
                if let Some(Value::String(filter_id)) = body.get("filter_id") {
                    log::debug!("filter_id = {}", filter_id);
                    Some(filter_id.to_string())
                } else {
                    log::error!("invalid response for get_filter");
                    None
                }
            } else {
                log::error!("invalid response for get_filter");
                None
            }
        } else {
            log::error!("Error for get_filter");
            None
        }
    } else {
        log::error!("Error unable to serialize request for get_filter");
        None
    }
}

fn get_messages(body: Map<String, Value>, room_id: &str) -> String {
    log::debug!("heap usage: {}", crate::cmds::heap_usage());
    let mut messages = String::new();
    if let Some(Value::Object(rooms)) = body.get("rooms") {
        if let Some(Value::Object(join)) = rooms.get("join") {
            if let Some(Value::Object(room)) = join.get(room_id) {
                if let Some(Value::Object(timeline)) = room.get("timeline") {
                    if let Some(Value::Array(events)) = timeline.get("events") {
                        for event in events.iter() {
                            if let Some(Value::String(type_)) = event.get("type") {
                                if type_.eq("m.room.message") {
                                    if messages.len() > 0 {
                                        messages.push_str("\n");
                                    }
                                    if let Some(Value::String(sender)) = event.get("sender") {
                                        messages.push_str(&get_username(sender));
                                    } else {
                                        messages.push_str("unknown");
                                    }
                                    messages.push_str("> ");
                                    if let Some(Value::Object(content)) = event.get("content") {
                                        if let Some(Value::String(body)) = content.get("body") {
                                            messages.push_str(body);

                                        } else {
                                            messages.push_str("....");
                                        }
                                    } else {
                                        messages.push_str("...");
                                    }
                                } // m.room.message
                            }
                        } // event
                    }
                }
            }
        }
    }
    messages
}

pub fn client_sync(server: &str, filter: &str, since: &str, timeout: i32,
                   room_id: &str, token: &str) -> Option<(String, String)> {
    log::debug!("heap usage: {}", crate::cmds::heap_usage());
    let mut url = String::from(server);
    url.push_str("/_matrix/client/r0/sync?filter=");
    url.push_str(filter);
    url.push_str("&timeout=");
    url.push_str(&timeout.to_string());
    if since.len() > 0 {
        url.push_str("&since=");
        url.push_str(since);
    }
    log::debug!("client_sync = {}", url);
    if let Some(value) = handle_response(get_json_auth(&url, token)) {
        if let Value::Object(body) = value {
            if let Some(Value::String(next_batch)) = body.get("next_batch") {
                Some((next_batch.to_string(), get_messages(body, room_id)))
            } else {
                log::error!("invalid response for client_sync");
                None
            }
        } else {
            log::error!("Error for client_sync: deserialization");
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
        MessageRequest {
            msgtype,
            body,
        }
    }
}

pub fn send_message(server: &str, room_id: &str, text: &str, txn_id: &str, token: &str) -> bool {
    log::debug!("heap usage: {}", crate::cmds::heap_usage());
    let room_id_encoded = url::encode(room_id);
    let mut url = String::from(server);
    url.push_str("/_matrix/client/r0/rooms/");
    url.push_str(&room_id_encoded);
    url.push_str("/send/m.room.message/");
    url.push_str(txn_id);
    log::debug!("send_message = {}", url);
    let message_request = MessageRequest::new(text);
    if let Some(request_body) = serialize(&message_request) {
        if let Some(value) = handle_response(put_string_auth(&url, &request_body, token)) {
            if let Value::Object(_body) = value {
                true
            } else {
                log::error!("invalid response for send_message");
                false
            }
        } else {
            log::error!("Error for send_message");
            false
        }
    } else {
        log::error!("Error unable to serialize request for send_message");
        false
    }
}
