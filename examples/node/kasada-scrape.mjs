import { connect, WreError } from "@proofofbot/runtime";

const PAGE = process.env.PAGE ?? "https://www.realestate.com.au/buy/in-sydney,+nsw/list-1";

const listings = (html) => {
  const found = new Set();
  const pattern = /href="(\/property-[^"]+)"/g;
  let match;

  while ((match = pattern.exec(html)) !== null) found.add(match[1]);

  return [...found];
};

const sidecar = await connect();
const session = await sidecar.open("kasada", { page_url: PAGE });

try {
  const surface = await session.call("discover", {});
  console.log(`${PAGE} answered ${surface.status}, protected ${surface.protected}`);

  if (!surface.protected) {
    console.log("no interrogation is being served, nothing to solve");
  } else {
    const solved = await session.call("solve", {}, { deadlineMs: 120000 });
    console.log(`verdict ${solved.verdict}, clearance ${solved.clearance}`);
    console.log(`payload ${solved.payload_bytes} bytes in ${solved.ms} ms`);

    const report = await session.call("report", {});
    console.log(`the agent flagged ${report.flagged.length} of its own checks`);
  }

  const page = await session.call("request", { url: PAGE }, { deadlineMs: 60000 });
  console.log(`page ${page.status}, ${page.bytes} bytes`);

  const found = listings(page.body);
  console.log(`${found.length} listings`);
  for (const listing of found.slice(0, 10)) console.log(`  https://www.realestate.com.au${listing}`);
} catch (error) {
  if (error instanceof WreError) {
    console.error(`${error.kind}: ${error.message}`);
    process.exitCode = 1;
  } else {
    throw error;
  }
} finally {
  await session.close();
  await sidecar.shutdown();
}
