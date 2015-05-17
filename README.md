# Coroutine I/O Demo

This is a demostration project for [coroutine-rs](https://github.com/rustcc/coroutine-rs).

## What is this for?

We are going to add a scheduler (work-stealing) into coroutine-rs, but we need to test before actually write it into the library.

## Goal

- [x] Asynchronous I/O with [MIO](https://github.com/carllerche/mio)

- [x] Single-threaded eventloop based Coroutine scheduler

- [ ] Multi-threaded eventloop based Coroutine sheduler (some weird bug need to be fixed)

- [ ] Network I/O library

- [ ] Synchronization between Coroutines (Mutex, CondVar, ...)

- [ ] Coroutine-local storage

- [ ] Windows support

## Known bugs

- [ ] Echo server may be blocked when enabling multi-thread mode
