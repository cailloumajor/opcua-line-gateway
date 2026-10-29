# TO-DO list for this project

## TBD

- [ ] Check that the browsed general part sheet has the expected nodes (by name),
names reference is the ClickHouse `data` subcolumns
- [ ] Support the new information model: machine object (`TraceabilityMachineType`)
with a heartbeat and several mailboxes (`TraceabilityMailboxType` subtypes), each
mailbox holding request, response, general part sheet and operation part sheet
- [ ] Handle traceability requests per mailbox (one subscription to the request
variable per mailbox), with the heartbeat at machine level
- [ ] Discover mailboxes by browsing the machine object (components whose type
derives from `TraceabilityMailboxType`), and the operation part sheet by type
(derived from `TraceabilityPartSheetOperationType`)
- [ ] Simplify machine configuration to the namespace URL and the machine object
node identifier
- [ ] At session initialization (and on each reconnection), build a
`BrowseName → NodeId` table per mailbox, using the operation namespace index;
keep reading and writing through numeric NodeIds at runtime
- [ ] Key cached general part sheet items by BrowseName (name part only, without
namespace index) instead of numeric node identifier
- [ ] Migrate or clear the existing redb cache when deploying the new cache format
- [ ] Decide the behavior on load when a cached BrowseName is missing in the
target mailbox (error, or warning and skip)
- [ ] Replace `part_id_nid`, `raw_part_ref_nid`, `raw_batch_nid` and
`excluded_read_nids` configuration by BrowseNames of the common namespace
- [ ] Keep the non-numeric NodeId check, and verify NodeId preservation on
SINUMERIK ONE for imported companion specifications

## DONE

- [x] Implement operation part sheet data flow (read and archive on save request)
