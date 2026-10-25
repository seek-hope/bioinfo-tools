---- MODULE CutadaptPipeline ----
EXTENDS Naturals, Sequences

CONSTANT Input
VARIABLES trimmed, written, queue

Init == trimmed = 0 /\ written = 0 /\ queue = <<>>

Trim == trimmed < Input /\ Len(queue) = 0 /\
        trimmed' = trimmed + 1 /\ written' = written /\ queue' = <<trimmed'>>

Write == Len(queue) > 0 /\ written < trimmed /\
         written' = written + 1 /\ trimmed' = trimmed /\ queue' = Tail(queue)

Done == trimmed = Input /\ written = trimmed /\ Len(queue) = 0 /\
        UNCHANGED <<trimmed, written, queue>>
Next == Trim \/ Write \/ Done
Spec == Init /\ [][Next]_<<trimmed, written, queue>>

NoLoss == written <= trimmed
BoundedQueue == Len(queue) <= 1
====
