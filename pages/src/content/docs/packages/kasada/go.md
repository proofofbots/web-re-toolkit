---
title: Kasada from Go
description: Install the Kasada client module, answer an interrogation, and fetch the page again through the same session with typed structs.
---

```bash
go get github.com/proofofbots/web-re-toolkit/packages/go/clients/kasada
```

Go 1.21 or later. Go modules publish from the repository tree by tag, so the module version follows the release tag.

A Kasada session mounts a graph profile. One is compiled into the binary, so there is nothing to capture before the first run. Capture your own with `wre sandbox capture --graph --open` and pass its id as `Profile` when you want a graph that is not shared with every other user, or one from a different browser.

```go
ctx := context.Background()

page := "https://acme.example/buy"
client, err := clientkasada.Open(ctx, &clientkasada.KasadaConfig{PageURL: &page}, clientkasada.OpenOptions{})
if err != nil {
	log.Fatal(err)
}
defer client.Close(ctx)

solved, err := client.Solve(ctx, clientkasada.SolveInput{})
if err != nil {
	log.Fatal(err)
}
fmt.Println(solved.Verdict, solved.PayloadBytes)
```

The token is bound to the `KP_UIDz` cookie the edge set on the interrogation, so solve against the url you actually want, then send everything else through the same client.

## A full run

Open one session, report what the page is serving, answer the interrogation, print how many of its own checks the agent flagged, then fetch the page again through the same session and list what came back. A session that never answered gets the interrogation instead of the page, which is the point of the comparison.

```go
package main

import (
	"context"
	"encoding/json"
	"log"
	"os"
	"regexp"
	"time"

	clientkasada "github.com/proofofbots/web-re-toolkit/packages/go/clients/kasada"
	"github.com/proofofbots/web-re-toolkit/packages/go/wre"
)

var listing = regexp.MustCompile(`href="(/property-[^"]+)"`)

func listings(html string) []string {
	seen := map[string]bool{}
	found := []string{}
	for _, match := range listing.FindAllStringSubmatch(html, -1) {
		if !seen[match[1]] {
			seen[match[1]] = true
			found = append(found, match[1])
		}
	}
	return found
}

func main() {
	page := os.Getenv("PAGE")
	if page == "" {
		page = "https://www.realestate.com.au/buy/in-sydney,+nsw/list-1"
	}

	ctx := context.Background()

	client, err := clientkasada.Open(ctx, &clientkasada.KasadaConfig{PageURL: &page}, clientkasada.OpenOptions{})
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close(ctx)

	surface, err := client.Discover(ctx, clientkasada.DiscoverInput{})
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("%s answered %d, protected %v", page, surface.Status, surface.Protected)

	if !surface.Protected {
		log.Println("no interrogation is being served, nothing to solve")
	} else {
		solveCtx, cancel := context.WithTimeout(ctx, 120*time.Second)
		solved, err := client.Solve(solveCtx, clientkasada.SolveInput{})
		cancel()
		if err != nil {
			if wre.IsKind(err, wre.KindBlocked) {
				log.Fatalf("blocked: %v", err)
			}
			log.Fatal(err)
		}

		clearance := ""
		if solved.Clearance != nil {
			clearance = *solved.Clearance
		}
		log.Printf("verdict %s, clearance %s", solved.Verdict, clearance)
		log.Printf("payload %d bytes in %d ms", solved.PayloadBytes, solved.Ms)

		report, err := client.Report(ctx)
		if err != nil {
			log.Fatal(err)
		}
		var flagged []json.RawMessage
		if err := json.Unmarshal(report.Flagged, &flagged); err != nil {
			log.Fatal(err)
		}
		log.Printf("the agent flagged %d of its own checks", len(flagged))
	}

	fetchCtx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()

	answered, err := client.Request(fetchCtx, clientkasada.RequestInput{URL: page})
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("page %d, %d bytes", answered.Status, answered.Bytes)

	found := listings(answered.Body)
	log.Printf("%d listings", len(found))
	for _, href := range found[:min(10, len(found))] {
		log.Printf("  https://www.realestate.com.au%s", href)
	}
}
```

Events, context deadlines, binary resolution and error kinds work the same for every target and are covered on the [Go package page](/web-re-toolkit/packages/go/). What the client does and what the config controls is in [The Kasada client](/web-re-toolkit/guides/kasada/).
