-- Reader 61. Historical observations without a writer-time generation stay unknown.
ALTER TABLE events ADD COLUMN observed_generation INTEGER CHECK(observed_generation >= 1);
