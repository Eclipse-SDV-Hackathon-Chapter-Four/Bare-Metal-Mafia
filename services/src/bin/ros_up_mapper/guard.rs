// Copyright (c) 2026 Contributors to the Bare-Metal-Mafia project
// See the NOTICE file(s) distributed with this work for additional
// information regarding copyright ownership.
//
// This program and the accompanying materials are made available under the
// terms of the Apache License, Version 2.0 which is available at
// https://www.apache.org/licenses/LICENSE-2.0
//
// AI Disclosure: This file was largely AI-generated. The AI-generated
// portions are made available under CC0-1.0 and not subject to the
// project's licence. The human contributor has reviewed and verified
// that the code is correct.
//
// SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
// Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

//! Runtime single-publisher guard.
//!
//! uProtocol publish events carry the topic as their source, not the
//! publisher, so a second publisher cannot be recognised by its address.
//! Instead the mapper subscribes to every uProtocol topic it publishes and
//! remembers a hash of each payload it sent; any received payload that is
//! not one of its own came from someone else.
//!
//! Default: log an error, count it in /stats and report /health unhealthy,
//! keep running. With ROS_UP_STRICT_SINGLE_PUBLISHER=1 the process exits
//! with code 1 instead (used by the tests).

use std::collections::{HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tracing::error;
use up_rust::{UListener, UMessage, UUri};

const REMEMBERED_PAYLOADS: usize = 1024;

pub struct Guard {
    strict: bool,
    sent: Mutex<(HashSet<u64>, VecDeque<u64>)>,
    pub foreign_events: AtomicU64,
}

impl Guard {
    pub fn from_env() -> Arc<Self> {
        let strict = matches!(
            std::env::var("ROS_UP_STRICT_SINGLE_PUBLISHER").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        Arc::new(Guard {
            strict,
            sent: Mutex::new((HashSet::new(), VecDeque::new())),
            foreign_events: AtomicU64::new(0),
        })
    }

    pub fn strict(&self) -> bool {
        self.strict
    }

    /// Record a payload this process is about to publish.
    pub fn remember(&self, payload: &[u8]) {
        let h = hash(payload);
        if let Ok(mut sent) = self.sent.lock() {
            if sent.0.insert(h) {
                sent.1.push_back(h);
                if sent.1.len() > REMEMBERED_PAYLOADS {
                    if let Some(old) = sent.1.pop_front() {
                        sent.0.remove(&old);
                    }
                }
            }
        }
    }

    fn is_own(&self, payload: &[u8]) -> bool {
        self.sent
            .lock()
            .map(|s| s.0.contains(&hash(payload)))
            .unwrap_or(false)
    }

    pub fn healthy(&self) -> bool {
        self.foreign_events.load(Ordering::Relaxed) == 0
    }

    /// Listener for one of our own output topics.
    pub fn watcher(self: &Arc<Self>, topic: UUri, label: String) -> Arc<dyn UListener> {
        Arc::new(Watcher {
            guard: self.clone(),
            topic,
            label,
        })
    }
}

fn hash(payload: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    payload.hash(&mut h);
    h.finish()
}

struct Watcher {
    guard: Arc<Guard>,
    topic: UUri,
    label: String,
}

#[async_trait]
impl UListener for Watcher {
    async fn on_receive(&self, message: UMessage) {
        let payload = message.payload.clone().unwrap_or_default();
        if self.guard.is_own(&payload) {
            return;
        }
        let n = self.guard.foreign_events.fetch_add(1, Ordering::Relaxed) + 1;
        error!(
            "SECOND PUBLISHER on {} ({}): received a message this mapper did not send ({} so far): {}",
            self.label,
            self.topic.to_uri(false),
            n,
            String::from_utf8_lossy(&payload)
        );
        if self.guard.strict {
            error!("ROS_UP_STRICT_SINGLE_PUBLISHER is set: exiting with code 1");
            std::process::exit(1);
        }
    }
}
