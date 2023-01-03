# About
I was interested in learning about distributed messaging protocols and stumbled along paxos.

This repo contains my attempt at implementing paxos in order to understand it.


# basic-paxos-lib 
Contains the paxos protocol.

It is only implemented for values of type usize.  This was to keep things simple while learning and since I don't have a current use for this library I haven't changed it.  Making this generic may be a low effort change and depends on SendToAcceptors.

It contains the Proposers and Acceptors, but no Learners.
Reason being I have seen a few different ways people propagate the decided values.  I felt this was the simplest.

# paxos_replicated_log_gui

![](./paxos_replicated_log_gui/docs/images/example.png)
This is the demo

It will create servers and allow the user to propose values.

It will also hold every message in a queue and allow the user to control the order of the messages.

# paxos-controllers
This is where all of the implementations of basic_paxos_lib::SendToAcceptors will live.  Or at least the ones I make