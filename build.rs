/*
 * Copyright 2021 Daniel Bornkessel
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;

use anyhow;
use anyhow::Context;
use serde_yaml;

fn keep_app_version_current_in_helm_chart() -> anyhow::Result<()> {
    const FILE: &str = "./charts/self-service-operators/Chart.yaml";

    let mut yaml: BTreeMap<String, String>;
    {
        let chart_file = File::open(FILE).context(format!("error opening {} for reading", FILE))?;

        yaml = serde_yaml::from_reader(&chart_file).context("error parsing yaml into btreemap")?;
        yaml.insert(
            "appVersion".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
    }

    let mut chart_file =
        File::create(FILE).context(format!("error opening {} for writing", FILE))?;
    write!(
        chart_file,
        "{}",
        serde_yaml::to_string(&yaml).context("error converting btreemap to string")?
    )
    .context(format!("error writing back file {}", FILE))?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    keep_app_version_current_in_helm_chart()
}
