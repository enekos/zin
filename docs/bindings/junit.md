---
title: JUnit 5
parent: Bindings
nav_order: 6
---

# JUnit 5
{: .no_toc }

`io.github.enekos:zin-junit` replays a specification's plan as JUnit Jupiter
dynamic tests. JDK 17+; depends on Jackson for JSON.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

The package is not on Maven Central yet. Install it from a checkout:

```sh
mvn -f bindings/junit/pom.xml install
```

then add the test dependencies:

```xml
<dependency>
  <groupId>io.github.enekos</groupId>
  <artifactId>zin-junit</artifactId>
  <version>0.6.0</version>
  <scope>test</scope>
</dependency>
<dependency>
  <groupId>org.junit.jupiter</groupId>
  <artifactId>junit-jupiter-engine</artifactId>
  <version>5.12.2</version>
  <scope>test</scope>
</dependency>
```

Install the `zin` binary (`cargo install zin-spec`) or commit plans.

## Minimal example

```java
import io.github.enekos.zin.Zin;
import java.io.IOException;
import java.util.Map;
import java.util.stream.Stream;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

class AccountTest {
    static class Harness implements Zin.Adapter {
        final Account account = new Account();

        @Override
        public boolean call(String action, Map<String, Object> args) {
            switch (action) {
                case "deposit" -> account.deposit((Long) args.get("amount"));
                case "close" -> { if (account.balance() != 0) return false; account.close(); }
                default -> throw new IllegalArgumentException(action);
            }
            return true;
        }

        @Override
        public Map<String, ?> observe() {
            return Map.of("balance", account.balance());
        }
    }

    @TestFactory
    Stream<DynamicTest> conforms() throws IOException {
        return Zin.load(Zin.resource(AccountTest.class, "account.zin")).tests(Harness::new);
    }
}
```

Put `account.zin` (and a committed `account.plan.json`, if you want one) in
`src/test/resources`; `Zin.resource` finds them on the test classpath as files.
`zin init junit account.zin` prints a skeleton.

## The adapter

```java
public interface Adapter extends AutoCloseable {
    boolean call(String action, Map<String, Object> arguments) throws Exception;
    default Map<String, ?> observe() throws Exception { return null; }
    default void close() throws Exception {}
}
```

| Member | Meaning |
| --- | --- |
| `call` | performs one action; `true` accepted, `false` refused (and nothing changed) |
| `observe` | any subset of state fields, or `null` to compare nothing |
| `close` | releases the implementation; always called, also when the test fails |

`tests(factory)` takes a `Supplier<? extends Adapter>`, usually a constructor
reference. Each dynamic test gets a fresh adapter, and a second one to replay a
shorter reproduction after a failure.

### How arguments arrive

By parameter name: integers as `Long`, booleans as `Boolean`, enum values as
`String`.

### How the implementation says no

Return `false`. **Any exception fails the test** with that exception; there is
no list of "rejecting" exception types. Catch your domain's refusal and return
`false`:

```java
case "withdraw" -> {
    try { account.withdraw((Long) args.get("amount")); }
    catch (InsufficientFunds refused) { return false; }
}
```

### What observe may return

A `Map<String, ?>` keyed by field name. Values are normalized:

| Java value | Compared as |
| --- | --- |
| `Byte`, `Short`, `Integer`, `Long` | an integer |
| `Boolean`, `String` | themselves |
| an enum constant | its `toString()` |
| `null`, empty `Optional` | nothing; a present `Optional` is its value |
| `Map` | a map (keys by their text) or a record; lists every key |
| a `record` | a record of its components |
| `Set` | a set, in any order |
| other `Collection` (`List`) | a list, in order |

`Map.of` rejects `null` values: build the observation in a `HashMap` when an
optional field can be nothing.

## Options

```java
Zin.load(spec, new Zin.Options().cover("pairs").pairLimit(50))
```

| `Zin.Options` method | Meaning |
| --- | --- |
| `cover(String)` | `transitions`, `states`, `actions`, `scenarios`, `pairs` |
| `maxSteps(int)` | soft tour length target |
| `stepBudget(int)` | hard limit on all steps |
| `pairLimit(int)` | sample size with `pairs` |
| `component(String)` | test one block of a composition |
| `plan(Path)` | an explicit plan file instead of the sibling `<name>.plan.json` |
| `binary(String)` | the `zin` binary (default `$ZIN_BIN`, then `zin` on `PATH`) |

Numeric options must be positive (`IllegalArgumentException: max_steps must be
a positive integer, not 0`). Committed plans follow
[the usual rules](index.md#where-the-plan-comes-from).

## API

| Member | Meaning |
| --- | --- |
| `Zin.load(Path spec)`, `Zin.load(Path spec, Options)` | the plan; throws `Zin.PlanException` (an `IOException`) |
| `Zin.resource(Class<?> anchor, String name)` | a classpath resource as a `Path` |
| `Zin.fingerprint(byte[])` | the FNV-1a 64 a plan records |
| `Plan.tests(Supplier<? extends Adapter>)` | `Stream<DynamicTest>`, one per plan test |
| `Plan.system()`, `fingerprint()`, `cover()` | what the plan was derived from |
| `Zin.Divergence` | an `AssertionError` with the report, `fingerprint()` and a pasteable `scenario()` |

## Running and filtering

Dynamic tests are named after the plan tests (`scenario: A reservation blocks
renewal`, `tour 1`). To replay one, filter the stream:

```java
@TestFactory
Stream<DynamicTest> tourOnly() throws IOException {
    return Zin.load(Zin.resource(LoanTest.class, "loan.zin"))
        .tests(Harness::new)
        .filter(test -> test.getDisplayName().equals("tour 1"));
}
```

## A complete example

The [Library loan](python.md#a-complete-example) specification, in
`src/test/resources/loan.zin`:

```java
// src/main/java/library/Book.java
package library;

import java.util.ArrayList;
import java.util.List;

public class Book {
    public String holder;
    public int renewals;
    public final List<String> waiting = new ArrayList<>();

    public boolean borrow(String member) {
        if (holder != null || (!waiting.isEmpty() && !waiting.contains(member))) return false;
        holder = member;
        waiting.remove(member);
        return true;
    }

    public boolean renew() {
        if (holder == null || !waiting.isEmpty() || renewals >= 2) return false;
        renewals++;
        return true;
    }

    public boolean giveBack() {
        if (holder == null) return false;
        holder = null;
        renewals = 0;
        return true;
    }

    public boolean reserve(String member) {
        if (holder == null || holder.equals(member) || waiting.contains(member)) return false;
        waiting.add(member);
        return true;
    }
}
```

```java
// src/test/java/library/LoanTest.java
package library;

import io.github.enekos.zin.Zin;
import java.io.IOException;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;
import java.util.stream.Stream;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

class LoanTest {
    static class Harness implements Zin.Adapter {
        final Book book = new Book();

        @Override
        public boolean call(String action, Map<String, Object> args) {
            return switch (action) {
                case "borrow" -> book.borrow((String) args.get("m"));
                case "renew" -> book.renew();
                case "give_back" -> book.giveBack();
                case "reserve" -> book.reserve((String) args.get("m"));
                default -> throw new IllegalArgumentException(action);
            };
        }

        @Override
        public Map<String, ?> observe() {
            Map<String, Object> state = new HashMap<>();
            state.put("holder", book.holder); // null is nothing
            state.put("renewals", book.renewals);
            state.put("waiting", Set.copyOf(book.waiting));
            return state;
        }
    }

    @TestFactory
    Stream<DynamicTest> loanConforms() throws IOException {
        return Zin.load(Zin.resource(LoanTest.class, "loan.zin")).tests(Harness::new);
    }
}
```

`mvn test` runs four dynamic tests. [`examples/pilots/junit`](https://github.com/enekos/zin/tree/main/examples/pilots/junit)
drives the JDK's `ByteBuffer`.

## Troubleshooting

| Message | Cause |
| --- | --- |
| `loan.zin is not on the test classpath` | the file is not under `src/test/resources` (or another resource root) |
| `... the zin binary was not found ...` | no usable plan, no binary |
| `... was derived from a different version of ...; regenerate it with: ...` | a stale explicit plan |
| `planning options need a .zin specification, not a plan file` | options with a `.json` path |
| your own exception | `call` threw instead of returning `false` |
