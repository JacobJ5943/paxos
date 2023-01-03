# About 
This is where the paxos protocol is defined.

Currently it's only defined for values of type usize.

In this version of paxos there are slots.  
In each slot a round of paxos will happen.

Once a value is decided for a slot it cannot be decided again.
The current slot being decided must always be increasing
