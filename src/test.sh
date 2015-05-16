#!/bin/sh

ulimit -Su 1000
ulimit -a

for i in $(seq 100); do
    echoping -n 5 -s 65535 -w 0.0001 127.0.0.1&
done

wait
