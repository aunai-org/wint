# Security

## Reporting a problem

Please report security problems privately, not in a public issue. On this repository open the **Security** tab and choose **Report a vulnerability**. Include the plan and data (or a small version of them) that triggers it, and what you saw. We will answer as soon as we can and fix confirmed problems before talking about them publicly.

## What counts

wint is written in safe Rust (the code has no `unsafe`), has no network access in the core, and does not store anything. The things worth reporting are:

- input that makes the library or the CLI crash, hang or use huge amounts of memory (plans, CSV, JSON or Open-Meteo responses are all treated as untrusted input);
- anything that lets data be read or written outside what you passed in;
- a problem in the WebAssembly package or the demo page, such as script injection through data or plan text.

The optional `net` feature fetches forecasts from Open-Meteo over HTTPS. Reports about that code path are welcome too.

## What does not count

- **A window you did not expect.** That is a bug, so please open a normal issue. wint ranks windows against the limits you set. Its presets are illustrative starting points and are **not safety guidance**; check limits against your own equipment and regulations.
- Forecasts being wrong. wint works with the data you give it.

## Supported versions

Only the latest release gets fixes while wint is at 0.x.
