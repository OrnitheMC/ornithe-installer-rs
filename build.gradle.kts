import java.util.regex.Pattern

plugins {
    `maven-publish`
}


group = "net.ornithemc"
val env: Map<String, String> = System.getenv()

val versionRegex: Pattern = Pattern.compile("version = \"(.*)\"")
version = if (env["SNAPSHOTS_URL"] != null) {
    "0-SNAPSHOT"
} else {
    file("Cargo.toml").useLines {
        it.map { versionRegex.matcher(it) }.first {

            it.matches()
        }.group(1)
    }
}

publishing {
    publications {
        val target = env["TARGET"]
        val os = env["OS"]

        /*
         * Note: this publication depends on files output by
         * cargo and environment variables currently only present in the
         * "Publish" github action. Running this outside the GHA environment WILL fail.
         */
        create<MavenPublication>("mavenCargo") {
            groupId = "net.ornithemc.ornithe-installer-rs"
            artifactId = "$os"

            artifact {
                file(
                    "$projectDir/target/$target/release/ornithe-installer-rs." + if (os?.contains("windows") == true) // thanks kotlin
                        "exe" else "bin"
                )
            }
            artifact (
                file(
                    "$projectDir/target/$target/release/ornithe-installer-rs-cli." + if (os?.contains("windows") == true) // thanks kotlin
                        "exe" else "bin"
                )
            ) {
                classifier = "cli"
            }
        }
    }

    repositories {
        if (env["MAVEN_URL"] != null) {
            repositories.maven {
                name = "Release"
                url = uri(env["MAVEN_URL"]!!)

                credentials {
                    username = env["MAVEN_USERNAME"]
                    password = env["MAVEN_PASSWORD"]
                }
            }
        }

        if (env["SNAPSHOTS_URL"] != null) {
            repositories.maven {
                name = "Snapshots"
                url = uri(env["SNAPSHOTS_URL"]!!)

                credentials {
                    username = env["SNAPSHOTS_USERNAME"]
                    password = env["SNAPSHOTS_PASSWORD"]
                }
            }
        }
    }
}

