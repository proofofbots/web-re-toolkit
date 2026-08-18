import { open } from "@proofofbot/client-akamai";

const PAGE = "https://matrix.leeclerk.org/";
const SEARCH = "https://matrix.leeclerk.org/Home/SearchByCaseNumber";
const CASE = process.env.CASE ?? "20tr456";

const rows = (html) => {
  const body = html.split("<tbody>")[1]?.split("</tbody>")[0] ?? "";

  return [...body.matchAll(/<tr[^>]*>([\s\S]*?)<\/tr>/g)].map((row) =>
    [...row[1].matchAll(/<td[^>]*>([\s\S]*?)<\/td>/g)].map((cell) =>
      cell[1].replace(/<[^>]*>/g, "").replace(/\s+/g, " ").trim()),
  );
};

const client = await open({ page_url: PAGE });

try {
  const solved = await client.solve({});
  console.log(`session ${solved.run.machine}, _abck ${solved.cookies.abck.status}`);

  const page = await client.page();
  const token = page.fields.__RequestVerificationToken;
  if (!token) throw new Error("the page carries no antiforgery token");

  const answer = await client.request({
    url: SEARCH,
    method: "POST",
    kind: "form",
    form: {
      __RequestVerificationToken: token,
      "byCaseNumber.CaseNumber": CASE,
      "byCaseNumber.CitationNumber": "",
      searchButton: "SearchByCaseNumber",
    },
  });

  const found = rows(answer.body);
  console.log(`search ${answer.status}, refused ${answer.refused}, ${found.length} matching`);

  for (const [number, citation, kind, status, filed] of found.slice(0, 5)) {
    console.log(`  ${number}  ${citation}  ${kind}  ${status}  ${filed}`);
  }
} finally {
  await client.close();
}
