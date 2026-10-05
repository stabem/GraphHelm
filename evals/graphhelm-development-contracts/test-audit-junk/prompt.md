---
max_turns: 4
allowed_tools: [Skill]
---

Use the test-audit skill in audit mode on these two tests and say, for each, whether it proves anything and which junk pattern it matches, if any.

Test 1:
```ts
test('total', () => {
  const expected = cartTotal([{p: 2, q: 3}]);
  expect(cartTotal([{p: 2, q: 3}])).toBe(expected);
});
```

Test 2:
```ts
test('fetches user', async () => {
  const api = { get: (id) => ({ id, name: id === 1 ? 'Ana' : 'Bo' }) };
  const svc = new UserService(api); // getName(id) returns api.get(id).name
  expect(await svc.getName(1)).toBe('Ana');
});
```
Answer only; do not edit anything.
