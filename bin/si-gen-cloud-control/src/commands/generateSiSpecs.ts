import { getServiceByName, loadCfDatabase } from "../cfDb.ts";
import { pkgSpecFromCf } from "../specPipeline.ts";
import { PkgSpec } from "../bindings/PkgSpec.ts";
import { generateAssetFuncs } from "../pipeline-steps/generateAssetFuncs.ts";
import { generateDefaultActionFuncs } from "../pipeline-steps/generateActionFuncs.ts";
import {
  generateSocketsFromDomainProps,
} from "../pipeline-steps/generateSocketsFromDomainProps.ts";

export function generateSiSpecForService(serviceName: string) {
  const cf = getServiceByName(serviceName);
  return pkgSpecFromCf(cf);
}

export async function generateSiSpecs() {
  const db = await loadCfDatabase();

  let imported = 0;
  const cfSchemas = Object.values(db);

  let specs = [] as PkgSpec[];

  for (const cfSchema of cfSchemas) {
    try {
      const pkg = pkgSpecFromCf(cfSchema);

      specs.push(pkg);
    } catch (e) {
      console.log(`Error Building: ${cfSchema.typeName}: ${e}`);
    }
  }

  // EXECUTE PIPELINE STEPS
  specs = generateSocketsFromDomainProps(specs);
  specs = generateAssetFuncs(specs);
  specs = generateDefaultActionFuncs(specs);

  // WRITE OUTS SPECS
  for (const spec of specs) {
    const specJson = JSON.stringify(spec, null, 2);
    const name = spec.name;

    try {
      await Deno.writeTextFile(
        `si-specs/${name}.json`,
        specJson,
      );
    } catch (e) {
      console.log(`Error writing to file: ${name}: ${e}`);
      continue;
    }

    imported += 1;
  }

  console.log(`built ${imported} out of ${cfSchemas.length}`);
}
