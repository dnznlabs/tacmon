#!/bin/sh
echo '{"ts":"2026-11-24T05:00:00.000Z","level":"alert","msg":"tick failed","why":"pool unreachable"}'
echo '{"ts":"2026-11-24T05:00:01.000Z","level":"info","msg":"tick failed","why":"x"}'
echo '{"ts":"2026-11-24T05:00:02.000Z","level":"info","msg":"fast bridge LP","poolAccount":"0xATTACKER0000000000000000000000000000beef","claimPub":"02a1b2c3d4e5f6","bitcoinAddress":"bc1qattacker"}'
echo '{"ts":"2026-11-24T05:00:03.000Z","level":"info","msg":"capacity","held":"999999900000000","promised":"0","open":0,"holds":0}'
exec sleep 60
