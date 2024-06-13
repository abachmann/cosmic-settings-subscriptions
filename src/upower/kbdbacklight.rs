// Copyright 2024 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use futures::SinkExt;
use std::{fmt::Debug, hash::Hash};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use upower_dbus::KbdBacklightProxy;

pub fn kbd_backlight_subscription<I: 'static + Hash + Copy + Send + Sync + Debug>(
    id: I,
) -> iced_futures::Subscription<KeyboardBacklightUpdate> {
    iced_futures::subscription::channel(id, 50, move |mut output| async move {
        let mut state = State::Ready;

        loop {
            state = start_listening(state, &mut output).await;
        }
    })
}

#[derive(Debug)]
pub enum State {
    Ready,
    Waiting(
        KbdBacklightProxy<'static>,
        UnboundedReceiver<KeyboardBacklightRequest>,
    ),
    Finished,
}

async fn get_brightness(kbd_proxy: &KbdBacklightProxy<'_>) -> zbus::Result<f64> {
    Ok(kbd_proxy.get_brightness().await? as f64 / kbd_proxy.get_max_brightness().await? as f64)
}

async fn start_listening(
    state: State,
    output: &mut futures::channel::mpsc::Sender<KeyboardBacklightUpdate>,
) -> State {
    match state {
        State::Ready => {
            let conn = match zbus::Connection::system().await {
                Ok(conn) => conn,
                Err(_) => return State::Finished,
            };
            let kbd_proxy = match KbdBacklightProxy::builder(&conn).build().await {
                Ok(p) => p,
                Err(_) => return State::Finished,
            };
            let (tx, rx) = unbounded_channel();

            let b = get_brightness(&kbd_proxy).await.ok();
            _ = output.send(KeyboardBacklightUpdate::Sender(tx)).await;
            _ = output.send(KeyboardBacklightUpdate::Brightness(b)).await;

            State::Waiting(kbd_proxy, rx)
        }
        State::Waiting(proxy, mut rx) => match rx.recv().await {
            Some(req) => match req {
                KeyboardBacklightRequest::Get => {
                    let b = get_brightness(&proxy).await.ok();
                    _ = output.send(KeyboardBacklightUpdate::Brightness(b)).await;
                    State::Waiting(proxy, rx)
                }
                KeyboardBacklightRequest::Set(value) => {
                    if let Ok(max_brightness) = proxy.get_max_brightness().await {
                        let value = value.clamp(0., 1.) * (max_brightness as f64);
                        let value = value.round() as i32;
                        let _ = proxy.set_brightness(value).await;
                    }

                    State::Waiting(proxy, rx)
                }
            },
            None => State::Finished,
        },
        State::Finished => futures::future::pending().await,
    }
}

#[derive(Debug, Clone)]
pub enum KeyboardBacklightUpdate {
    Sender(UnboundedSender<KeyboardBacklightRequest>),
    Brightness(Option<f64>),
}

#[derive(Debug, Clone)]
pub enum KeyboardBacklightRequest {
    Get,
    Set(f64),
}
