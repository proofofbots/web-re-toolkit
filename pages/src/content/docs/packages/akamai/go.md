---
title: Akamai from Go
description: Install the Akamai client module, warm a session against a protected page, and post a form through the same cookie jar with typed structs.
---

```bash
go get github.com/proofofbots/web-re-toolkit/packages/go/clients/akamai
```

Go 1.21 or later. Go modules publish from the repository tree by tag, so the module version follows the release tag.

```go
ctx := context.Background()

page := "https://acme.example/"
client, err := clientakamai.Open(ctx, &clientakamai.AkamaiConfig{PageURL: &page}, clientakamai.OpenOptions{})
if err != nil {
	log.Fatal(err)
}
defer client.Close(ctx)

solved, err := client.Solve(ctx, clientakamai.SolveInput{})
if err != nil {
	log.Fatal(err)
}
fmt.Println(string(solved.Cookies))
```

Each op is a method with typed input and result structs, generated from the descriptor.

## A full run

Warm a session against a protected login page, read the antiforgery token out of the page the session already loaded, and post a form through the same jar.

```go
package main

import (
	"context"
	"fmt"
	"log"
	"strings"
	"time"

	clientakamai "github.com/proofofbots/web-re-toolkit/packages/go/clients/akamai"
)

const (
	page     = "https://login.xero.com/identity/user/login"
	precheck = "https://login.xero.com/identity/user/login/pre-check"
)

func field(html, name string) string {
	at := strings.Index(html, fmt.Sprintf(`name=%q`, name))
	if at < 0 {
		return ""
	}
	rest := html[at:]
	start := strings.Index(rest, `value="`)
	if start < 0 {
		return ""
	}
	tail := rest[start+7:]
	end := strings.Index(tail, `"`)
	if end < 0 {
		return ""
	}
	return tail[:end]
}

func main() {
	ctx := context.Background()

	pageURL := page
	waitMs := int64(100)
	rounds := int64(1)

	client, err := clientakamai.Open(ctx, &clientakamai.AkamaiConfig{
		PageURL: &pageURL,
		WaitMs:  &waitMs,
		Rounds:  &rounds,
	}, clientakamai.OpenOptions{})
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close(ctx)

	found, err := client.Discover(ctx, clientakamai.DiscoverInput{})
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("discover: status %d protected %v", found.Status, found.Protected)

	solved, err := client.Solve(ctx, clientakamai.SolveInput{})
	if err != nil {
		log.Fatal(err)
	}
	payloadBytes := 0
	if solved.Payload != nil {
		payloadBytes = len(*solved.Payload)
	}
	log.Printf("solve: payload %d bytes, posts %s", payloadBytes, solved.Posts)

	state, err := client.Page(ctx)
	if err != nil {
		log.Fatal(err)
	}

	html := state.HTML
	if html == "" {
		fetched, err := client.Request(ctx, clientakamai.RequestInput{URL: page})
		if err != nil {
			log.Fatal(err)
		}
		html = fetched.Body
	}

	token := state.Fields["__RequestVerificationToken"]
	if token == "" {
		token = field(html, "__RequestVerificationToken")
	}
	if token == "" {
		log.Fatal("no antiforgery token")
	}

	returnURL := state.Fields["ReturnUrl"]
	if returnURL == "" {
		returnURL = field(html, "ReturnUrl")
	}

	username := fmt.Sprintf("nx%x@example.com", time.Now().Unix())
	method := "POST"

	if _, err := client.Request(ctx, clientakamai.RequestInput{
		URL:    precheck,
		Method: &method,
		JSON:   []byte(fmt.Sprintf(`{"Username":%q}`, username)),
		Headers: map[string]string{
			"accept":                   "application/json, text/plain, */*",
			"origin":                   "https://login.xero.com",
			"requestverificationtoken": token,
		},
	}); err != nil {
		log.Fatal(err)
	}

	answer, err := client.Request(ctx, clientakamai.RequestInput{
		URL:    page,
		Method: &method,
		Form: map[string]string{
			"ReturnUrl":                  returnURL,
			"PreCheckCompleted":          "true",
			"Username":                   username,
			"Password":                   "Nx7!aQ2zR9kL",
			"__RequestVerificationToken": token,
		},
		Headers: map[string]string{
			"accept":                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
			"origin":                    "https://login.xero.com",
			"sec-fetch-dest":            "document",
			"sec-fetch-mode":            "navigate",
			"sec-fetch-site":            "same-origin",
			"upgrade-insecure-requests": "1",
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	body := strings.ToLower(answer.Body)
	log.Printf("login: status %d refused %v credential_error %v",
		answer.Status,
		answer.Refused,
		strings.Contains(body, "email address or password") || strings.Contains(body, "incorrect"))
}
```

`Discover` reports the surface without running the sensor, so it is the cheapest way to tell whether a page is protected. `Page` returns the document the session last loaded along with every input it declares, which saves a second fetch. `Refused` is true on a 403, a 429, an access denied body or a challenge redirect, so a `false` there with a credential error in the body means the session passed and the login itself was rejected.

Events, context deadlines, binary resolution and error kinds work the same for every target and are covered on the [Go package page](/web-re-toolkit/packages/go/). What the client does and what the config controls is in [The Akamai client](/web-re-toolkit/guides/akamai/).
