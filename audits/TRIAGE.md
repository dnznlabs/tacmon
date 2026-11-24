# Triage

What was done about every finding of every run, newest run first. A run's `AUDIT.md` is never edited; this is
where its findings are answered. **Fixed** names the test that fails without the fix. **Accepted** gives the
reason and where the limit is written down. **Open** is still to do.

## 2026-11-24-2

A re-run on the fixed code (`85014a2`), one leg. It found no high finding and broke claims 4 and 9 again, two of
them through gaps in the first round's own fixes.

| # | Finding | Outcome | Held by |
|---|---|---|---|
| F-1 | A redirect with a body is read as an answer | **Fixed.** Only a 2xx is an answer; `/health` reads an error body but never a redirect's. The first fix's test passed only because its redirect had no body | `http::tests::a_redirect_is_an_error_and_is_not_followed`, `http::tests::health_is_not_read_from_the_body_of_a_redirect` |
| F-2 | Market and Advisor draw stale board data as current; expired wants kept | **Fixed.** Both titles say when the board does not answer, and a want is dropped when its time passes | `state::tests::a_want_stops_waiting_when_its_time_passes_without_a_new_answer`; the titles are not under test |
| F-3 | Log figures carry no source or age; no capacity line drawn as 0.00 | **Fixed.** Promised and held carry the log's age; before any capacity line they are a dash | `state::tests::free_is_not_known_before_anything_has_said`; the drawing is not under test |
| F-4 | A lone forged ad under the key lights "on board"; unknown key shows red | **Fixed.** An ad is this provider's only when it carries the terms `/info` reports; any other under the key is flagged; the light is unknown until the key is known | `state::tests::a_lone_ad_under_the_key_with_other_terms_is_not_this_provider`, `state::tests::on_board_is_unknown_until_the_key_is_known` |
| F-5 | Missing swap and want fields become zeros | **Fixed.** A record whose offer lacks an amount is not counted; a missing start or expiry is a dash; an unreadable amount in a log line is shown as written | `wire::tests::record_whose_offer_lacks_an_amount_is_not_counted_as_a_swap_of_zero`, `wire::tests::an_amount_that_cannot_be_read_is_shown_as_written_not_as_zero`; the dashes are not under test |
| F-6 | One of several boards failing is invisible; duplicates counted twice | **Open.** Only with more than one board configured | |
| F-7 | Board answers inflate about 50x in memory; the Advisor counts fees on dust | **Fixed.** A board list over 5,000 entries is a failed read, and a bridge smaller than its own fee earns nothing | `http::tests::a_flooded_board_list_is_a_failed_read`, `advisor::tests::a_bridge_smaller_than_its_fee_earns_nothing` |
| F-8 | The log store can hold hundreds of MiB | **Fixed.** A kept line is cut at 2 KiB, so 5,000 of them hold about 20 MiB at most | `wire::tests::a_long_line_is_kept_short_and_cut_on_a_character` |
| F-9 | Unbounded channels; swap file check-then-read | **Open.** The file part needs write access to the data directory | |
| F-10 | "System proxy" wording wrong; a proxy can answer for the local provider | **Fixed in part.** Claim 4 now names the environment variables. Loopback is still sent through a proxy that is set without `NO_PROXY` | `SECURITY.md` |
| F-11 | Claim 7 rests on ratatui behaviour nothing pins | **Fixed.** A test draws escape, bell and other control characters and reads the buffer | `widgets::tests::control_characters_in_drawn_text_never_reach_the_terminal` |
| F-12 | The TLS stack compiles C | **Accepted.** Claim 3 is about this crate | |
| F-13 | Repeat counter wrap; `reverse` loose; identity from the log; ETH through `f64` | **Accepted**, display only or unreachable; identity from the log as in the first run's F9 | |

Not re-reviewed: these fixes have been checked by their tests only.

## 2026-11-24

One leg, so every finding was unconfirmed when triaged. Each fix was checked by its test, not by a second review.

| # | Finding | Outcome | Held by |
|---|---|---|---|
| F1 | A log read error ends the feed silently | **Fixed.** Bytes that are not UTF-8 are shown, not fatal; a read error stops the command and restarts it with a warning | `local::tests::bytes_that_are_not_utf8_do_not_end_the_feed` |
| F2 | Redirects followed to any host | **Fixed.** A redirect is an error | `http::tests::a_redirect_is_an_error_and_is_not_followed` |
| F3 | An ad under the operator's key takes the name and the light | **Fixed in part.** A second ad under the key is flagged, and the name and the light follow the ad with the provider's own terms. An impostor copying the terms exactly is still indistinguishable | `state::tests::a_second_ad_under_the_same_key_does_not_take_the_name`; limit in `SECURITY.md` |
| F4 | A later info line overwrites an alert | **Fixed.** Lines fold only at the same level. Details under one message at one level still fold to the newest, by design | `state::tests::an_alert_is_not_folded_into_a_quieter_line_with_the_same_words` |
| F5 | No size cap on answers | **Fixed.** 4 MiB | `http::tests::a_body_over_the_limit_is_refused_before_it_is_kept` |
| F6 | Sums and fees wrap; casts truncate | **Fixed.** Saturating arithmetic and checked narrowing throughout | `model::tests::amounts_too_large_to_hold_stop_at_the_top_and_never_wrap`, `wire::tests::numbers_too_large_to_hold_are_not_cut_to_their_low_bits` |
| F7 | A failed status or missing field drawn as zero terms | **Fixed.** Both are failed reads; an ad without terms is left out | `http::tests::a_failed_status_is_a_failed_read_not_zero_terms`, `wire::tests::answer_without_terms_is_not_read_as_zero_terms`, `wire::tests::ad_without_terms_is_left_out` |
| F8 | An unreadable fee cap read as "any fee" | **Fixed.** It is the strictest cap | `wire::tests::fee_cap_that_cannot_be_read_is_the_strictest_cap` |
| F9 | Log lines replace identity and capacity; `free` falls back to the log with no age | **Fixed in part.** A figure from the log is drawn as such, in the warning colour, with its age; a capacity line without amounts is ignored. Identity is still taken from the provider's own log: that log is the provider's word | `state::tests::free_read_from_the_log_says_so_and_how_old_it_is`, `wire::tests::capacity_line_without_its_amounts_carries_no_signal` |
| F10 | Health comes from whatever owns the port | **Accepted.** tacmon has nothing to check the answer against; needs another user on the machine | `SECURITY.md` § What this does not cover |
| F11 | Right-aligned columns drop leading digits | **Open.** Now needs an absurd amount to reach, since sums no longer wrap, but the cut is still unmarked | |
| F12 | Stale or partial board data drawn as fresh | **Fixed in part.** A failed read of ads now turns the "on board" light to unknown. One board of several failing is still silent, and an entry on two boards is counted twice | `state::tests::ads_that_cannot_be_read_mark_the_board_as_not_answering` |
| F13 | Swap records skipped silently; a pipe or device hangs the read | **Fixed in part.** Only ordinary files up to 1 MiB are read. A malformed record is still skipped without a count | none: needs a filesystem fixture |
| F14 | Log lines have no length limit | **Fixed.** 64 KiB, the rest dropped | `local::tests::a_line_with_no_end_is_cut_and_the_next_one_is_still_read` |
| F15 | Expired wants count as demand | **Fixed** | `state::tests::wants_past_their_time_are_not_counted_as_waiting` |
| F16 | Unicode format characters reach the terminal | **Accepted.** Visual only | `SECURITY.md` § What this does not cover |
| F17 | System proxy honoured; `User-Agent`; lights differ by colour only; C build script; start-up `expect` | **Accepted**, except the proxy, now stated in claim 4. The failed light sharing a glyph with the healthy one is **fixed**: a failure is a cross | not under test |

Claims after triage: 4, 8 and 9 were reworded to say what the code now does and are held by tests. Claim 2 stands
again with redirects refused. None of this has been reviewed by a second leg.
