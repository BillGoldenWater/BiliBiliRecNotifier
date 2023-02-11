use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use notify_rust::NotificationHandle;
use serde::{Deserialize, Serialize};

static mut ROOMID_FILTER: Option<Vec<u32>> = None;

#[tokio::main]
async fn main() {
  let mut args: Args = argh::from_env();
  let roomid_filter = args.roomid_filter.as_ref().map(|it| {
    it.split(',')
      .filter_map(|it| u32::from_str(it).ok())
      .collect::<Vec<_>>()
  });
  if roomid_filter.is_some() {
    unsafe {
      args.roomid_filter = roomid_filter.as_ref().map(|it| {
        it.iter()
          .map(|it| it.to_string())
          .collect::<Vec<_>>()
          .join(", ")
      });
      ROOMID_FILTER = roomid_filter;
    }
  }

  println!("run with {args:#?}");
  run_server(args.port).await;
}

fn notify(event: Event) -> notify_rust::error::Result<NotificationHandle> {
  #[cfg(target_os = "macos")]
  static SOUND: &str = "Submarine";

  #[cfg(all(unix, not(target_os = "macos")))]
  static SOUND: &str = "message-new-instant";

  #[cfg(target_os = "windows")]
  static SOUND: &str = "Mail";

  notify_rust::Notification::new()
    .summary("Live started!")
    .body(&format!(
      "Room {room} is streaming.\n\n{title}",
      room = event.event_data.room_id,
      title = event.event_data.title
    ))
    .sound_name(SOUND)
    .show()
}

#[derive(argh::FromArgs, Debug)]
/// Settings
struct Args {
  /// webhook listen port
  #[argh(option, default = "25550")]
  port: u16,
  /// a list of roomid that need send notification split by ','
  #[argh(option)]
  roomid_filter: Option<String>,
}

async fn run_server(port: u16) {
  // We'll bind to 127.0.0.1:3000
  let addr = SocketAddr::from(([0, 0, 0, 0], port));

  // A `Service` is needed for every connection, so this
  // creates one from our `hello_world` function.
  let make_svc = make_service_fn(|_conn| async {
    // service_fn converts our function into a `Service`
    Ok::<_, Infallible>(service_fn(handle_request))
  });

  let server = Server::bind(&addr).serve(make_svc);

  // And now add a graceful shutdown signal...
  let graceful = server.with_graceful_shutdown(shutdown_signal());

  println!("server started");

  // Run this server for... forever!
  if let Err(e) = graceful.await {
    eprintln!("server error: {e}");
  }

  println!("server stopped");
}

async fn handle_request(req: Request<Body>) -> Result<Response<Body>, Infallible> {
  println!(
    "{} {} {:?}",
    req.method().as_str(),
    req.uri(),
    req.version()
  );
  if req.method() != Method::POST {
    println!("invalid method");
    return not_found();
  }

  if req.uri().path() != "/webhook" {
    println!("invalid path");
    return not_found();
  }

  let body = hyper::body::to_bytes(req.into_body()).await;
  let body = match body {
    Ok(body) => body,
    Err(err) => {
      println!("failed to read body\n{err:#?}");
      return server_err(format!("{err:#?}"));
    }
  };

  let event = serde_json::from_slice::<Event>(body.as_ref());
  let event = match event {
    Ok(event) => event,
    Err(err) => {
      println!("failed to parse body\n{err:#?}");
      return server_err(format!("{err:#?}"));
    }
  };

  if event.event_type == "StreamStarted" {
    unsafe {
      if ROOMID_FILTER.is_some()
        && !ROOMID_FILTER
          .as_ref()
          .unwrap()
          .contains(&(event.event_data.room_id as u32))
      {
        println!("{} ignored", event.event_data.room_id);
        return Ok(Response::new(Body::empty()));
      }
    }
    let result = notify(event);

    if let Err(err) = result {
      println!("failed to show notification\n{err:#?}");
      return server_err(format!("{err:#?}"));
    }
  }

  println!("success");
  Ok(Response::new(Body::empty()))
}

fn not_found() -> Result<Response<Body>, Infallible> {
  Ok(
    Response::builder()
      .status(StatusCode::NOT_FOUND)
      .body(Body::empty())
      .unwrap(),
  )
}

fn server_err(msg: String) -> Result<Response<Body>, Infallible> {
  Ok(
    Response::builder()
      .status(StatusCode::INTERNAL_SERVER_ERROR)
      .body(Body::from(msg))
      .unwrap(),
  )
}

async fn shutdown_signal() {
  // Wait for the CTRL+C signal
  tokio::signal::ctrl_c()
    .await
    .expect("failed to install CTRL+C signal handler");
}

#[derive(Serialize, Deserialize)]
struct EventData {
  #[serde(rename = "RoomId")]
  pub room_id: i64,
  #[serde(rename = "ShortId")]
  pub short_id: i64,
  #[serde(rename = "Name")]
  pub name: String,
  #[serde(rename = "Title")]
  pub title: String,
  #[serde(rename = "AreaNameParent")]
  pub area_name_parent: String,
  #[serde(rename = "AreaNameChild")]
  pub area_name_child: String,
  #[serde(rename = "Recording")]
  pub recording: bool,
  #[serde(rename = "Streaming")]
  pub streaming: bool,
  #[serde(rename = "DanmakuConnected")]
  pub danmaku_connected: bool,
}

#[derive(Serialize, Deserialize)]
struct Event {
  #[serde(rename = "EventType")]
  pub event_type: String,
  #[serde(rename = "EventTimestamp")]
  pub event_timestamp: String,
  #[serde(rename = "EventId")]
  pub event_id: String,
  #[serde(rename = "EventData")]
  pub event_data: EventData,
}
