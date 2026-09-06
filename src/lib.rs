// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! JII — Just Install It: a smart universal package installer for Linux.
//!
//! This is the library half of the crate; `src/main.rs` is a thin binary over it. Both live
//! in one Cargo crate (ADR-0001) — a `[lib]` and a `[[bin]]` target, not a workspace — so the
//! split costs no build graph and buys three things: the module boundaries become a real,
//! documented API surface rather than an accident of `mod` ordering; integration tests under
//! `tests/` can drive JII the way a caller would instead of only through a spawned process;
//! and `cargo doc` produces something worth reading.
//!
//! The shape, top to bottom:
//!
//! - [`cli`] — the command surface: parse, dispatch, and every user-facing flow.
//! - [`engine`] — search, ranking, planning and execution over the provider set.
//! - [`provider`] — one module per source, all behind the [`provider::Provider`] trait. The
//!   core never branches on a concrete source id (ADR-0004).
//! - [`model`] — the source-agnostic domain: candidates, plans, actions, trust.
//! - [`ui`] — all presentation, including the house voice in [`ui::story`] (ADR-0089).
//! - [`platform`], [`privilege`], [`exec`] — the machine, escalation, and running things.
//! - [`config`], [`registry`], [`cache`], [`secret`] — state on disk.
//! - [`i18n`], [`topics`], [`recommend`], [`changelog`] — the data-driven, translated layers.
//!
//! See `docs/ARCHITECTURE.md` for the reasoning behind all of it.

pub mod achievements;
pub mod cache;
pub mod changelog;
pub mod cli;
pub mod config;
pub mod devtest;
pub mod engine;
pub mod error;
pub mod exec;
pub mod i18n;
pub mod model;
pub mod platform;
pub mod privilege;
pub mod progress;
pub mod provider;
pub mod recommend;
pub mod registry;
pub mod secret;
pub mod selfupdate;
pub mod shellrc;
pub mod topics;
pub mod ui;
