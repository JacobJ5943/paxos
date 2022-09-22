This server is going to have a producer and an acceptor

It will have a single client accessable endpoint which will be used to propose a value


# Who owns the proposer and acceptor?
1. use an Arc Mutex
2. Have a second sharable struct which has a channel system to another thread.  That other thread has exclusive access to the proposer.
