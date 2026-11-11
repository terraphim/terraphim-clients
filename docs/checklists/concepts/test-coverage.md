# Tests

A test earns its green tick only if it fails when the behaviour it guards is removed. Two suites
can be the same size and only one can be worth running: the difference is whether reading the
test tells you what breaks when it goes red.

Coverage is a guide to what has been exercised, never a measure of whether the assertions mean
anything; a test that reimplements the logic it checks will pass whatever the production code
does.

synonyms:: tests, testing, test suite, regression test, coverage, fixture, assertion
