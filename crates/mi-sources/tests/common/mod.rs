//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::sync::Arc;

use mi_sources::http::Request;
use mi_sources::testing::FixtureTransport;
use mi_sources::{ApiKeys, Cache, HttpClient, Sources};
use mi_types::ProviderId;

/// The public URL the code builds for `base` with `params` (same encoding and order).
pub fn url(provider: ProviderId, base: &str, params: &[(&str, &str)]) -> String {
    Request::get(provider, base, params)
        .unwrap()
        .public_url()
        .to_owned()
}

/// A fixture file's text.
pub fn fixture(path: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

/// Sources over `transport` with an in-memory cache.
pub fn sources(transport: &Arc<FixtureTransport>, keys: ApiKeys) -> Sources {
    let http = HttpClient::with_transport(transport.clone());
    Sources::new(http, Cache::in_memory().unwrap(), keys)
}

pub const TVMAZE: &str = "https://api.tvmaze.com";

/// Registers TVmaze fixtures for Schoolhouse Rock! (show 15448).
pub fn schoolhouse_rock(t: &FixtureTransport) {
    t.on_json(
        &url(
            ProviderId::Tvmaze,
            &format!("{TVMAZE}/search/shows"),
            &[("q", "Schoolhouse Rock")],
        ),
        &fixture("tvmaze/search-schoolhouse-rock.json"),
    );
    t.on_json(
        &url(ProviderId::Tvmaze, &format!("{TVMAZE}/shows/15448"), &[]),
        &fixture("tvmaze/schoolhouse-rock-show.json"),
    );
    t.on_json(
        &url(
            ProviderId::Tvmaze,
            &format!("{TVMAZE}/shows/15448/episodes"),
            &[("specials", "1")],
        ),
        &fixture("tvmaze/schoolhouse-rock-episodes.json"),
    );
    t.on_json(
        &url(
            ProviderId::Tvmaze,
            &format!("{TVMAZE}/shows/15448/alternatelists"),
            &[],
        ),
        "[]",
    );
}

/// Registers TVmaze fixtures for Firefly (show 180), including its DVD list.
pub fn firefly(t: &FixtureTransport) {
    t.on_json(
        &url(ProviderId::Tvmaze, &format!("{TVMAZE}/shows/180"), &[]),
        &fixture("tvmaze/firefly-show.json"),
    );
    t.on_json(
        &url(
            ProviderId::Tvmaze,
            &format!("{TVMAZE}/shows/180/episodes"),
            &[("specials", "1")],
        ),
        &fixture("tvmaze/firefly-episodes.json"),
    );
    t.on_json(
        &url(
            ProviderId::Tvmaze,
            &format!("{TVMAZE}/shows/180/alternatelists"),
            &[],
        ),
        &fixture("tvmaze/firefly-alternatelists.json"),
    );
    t.on_json(
        &url(
            ProviderId::Tvmaze,
            &format!("{TVMAZE}/alternatelists/1/alternateepisodes"),
            &[("embed", "episodes")],
        ),
        &fixture("tvmaze/firefly-alternateepisodes.json"),
    );
}

/// A ZIP archive holding `files`.
pub fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in files {
            w.start_file(*name, options).unwrap();
            w.write_all(content.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// A one-cue SubRip file saying `line`.
pub fn srt(line: &str) -> String {
    format!("1\n00:00:01,000 --> 00:00:03,000\n{line}\n")
}
