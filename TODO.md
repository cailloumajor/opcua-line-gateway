# TO-DO list for this project

## TBD

- [ ] Check that the browsed general part sheet has the expected nodes (by name),
names reference is the ClickHouse `data` subcolumns
- [ ] Migrate or clear the existing redb cache when deploying the new cache format
- [ ] Verify NodeId preservation on SINUMERIK ONE for imported companion
specifications

## DONE

- [x] Implement operation part sheet data flow (read and archive on save request)
- [x] Support the [new information model](docs/information-model.md): machine
object (`TraceabilityMachineType`) with a heartbeat and several mailboxes
(`TraceabilityMailboxType` subtypes), each mailbox holding request, response,
general part sheet and operation part sheet
- [x] Handle traceability requests per mailbox (one subscription to the request
variable per mailbox), with the heartbeat at machine level
- [x] Discover mailboxes by browsing the machine object (components whose type
derives from `TraceabilityMailboxType`), and the operation part sheet by type
(derived from `TraceabilityPartSheetOperationType`)
- [x] Resolve all the nodes of a machine with a single recursive `Browser` run
from the machine object (one `Browse` request per level), then initialize each
mailbox from its part of the results; identify the nodes with BrowseNames
defined by the common namespace types (heartbeat, and request, response and
general part sheet of each mailbox) by BrowseName, and report the missing ones
- [x] Browse the part sheets variables as data variables (`HasComponent`
references to nodes of `Variable` class) instead of properties (`HasProperty`
references)
- [x] Simplify machine configuration to the namespace URL and the machine object
node identifier
- [x] At session initialization (and on each reconnection), build a
`BrowseName → NodeId` table per mailbox, using the operation namespace index;
keep reading and writing through numeric NodeIds at runtime
- [x] Key cached general part sheet items by BrowseName (name part only, without
namespace index) instead of numeric node identifier
- [x] Decide the behavior on load when a cached BrowseName is missing in the
target mailbox: the item is skipped
- [x] Replace `part_id_nid`, `raw_part_ref_nid`, `raw_batch_nid` and
`excluded_read_nids` configuration by BrowseNames of the common namespace
- [x] Keep the non-numeric NodeId check
