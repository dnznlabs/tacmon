#!/bin/sh
echo '{"ts":"2026-11-24T05:00:00.000Z","level":"info","msg":"before the bad byte"}'
printf 'garbage \377 byte\n'
echo '{"ts":"2026-11-24T05:00:01.000Z","level":"alert","msg":"AFTER the bad byte: never shown"}'
exec sleep 60
