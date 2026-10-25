---- MODULE BamSort ----
EXTENDS Naturals, Sequences

CONSTANT Records
VARIABLES read, spilled, merged

Init == read = 0 /\ spilled = 0 /\ merged = 0

Read == read < Records /\ read' = read + 1 /\ spilled' = spilled /\ merged' = merged
Spill == read = Records /\ spilled < read /\ spilled' = spilled + 1 /\ read' = read /\ merged' = merged
Merge == spilled = read /\ merged < spilled /\ merged' = merged + 1 /\ read' = read /\ spilled' = spilled

Done == read = Records /\ spilled = read /\ merged = spilled /\
        UNCHANGED <<read, spilled, merged>>
Next == Read \/ Spill \/ Merge \/ Done
Spec == Init /\ [][Next]_<<read, spilled, merged>>

NoLoss == merged <= read
NoDuplicate == merged <= spilled
====
