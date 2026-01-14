## 2025-02-25 - [Redundant Expensive Calculations in React Components]
**Learning:** React components often perform redundant expensive calculations (like image processing) in `useEffect` that are already done elsewhere (e.g., in a global store). Consolidating these into the store avoids duplicate work and leverages caching.
**Action:** When seeing expensive effects in components, check if the data can be derived from existing global state or computed once in the store.
