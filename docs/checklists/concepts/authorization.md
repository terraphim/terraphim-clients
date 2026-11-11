# Authorization

Authorization decides what an authenticated caller is *allowed* to do. It is where multi-tenancy
bugs live: a request that carries an object id and trusts it, a query that filters by tenant in
the list path but not the detail path, a role check that runs in the UI but not the API.

The durable review pattern is least privilege plus server-side checks on every path that crosses
a boundary, including the ones the UI never renders.

synonyms:: authorization, access control, permissions, rbac, least privilege, role check, tenant isolation
