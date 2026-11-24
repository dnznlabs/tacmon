# Triage

What was done about every finding of every run, newest run first. A run's `AUDIT.md` is never edited; this is
where its findings are answered. **Fixed** names the test that fails without the fix. **Accepted** gives the
reason and where the limit is written down. **Open** is still to do.

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
| F17 | System proxy honoured; `User-Agent`; lights differ by colour only; C build script; start-up `expect` | **Accepted**, except the proxy, now stated in claim 4. The failed light sharing a glyph with the healthy one is **open** | |

Claims after triage: 4, 8 and 9 were reworded to say what the code now does and are held by tests. Claim 2 stands
again with redirects refused. None of this has been reviewed by a second leg.
