plugins {
    java
    id("com.gradleup.shadow") version "9.3.+"
    id("xyz.wagyourtail.jvmdowngrader") version "1.3.4"
}

java {
    targetCompatibility = JavaVersion.VERSION_21
    sourceCompatibility = JavaVersion.VERSION_21
}

base {
    archivesName = "ServerLauncher"
}

repositories {
    mavenCentral()
}

dependencies {
    compileOnly("com.google.code.gson:gson:2.10")
    compileOnly("org.slf4j:slf4j-api:2.0.1")
}

jvmdg {
    downgradeTo = JavaVersion.VERSION_1_8
    multiReleaseOriginal.set(true)
    multiReleaseVersions.set(listOf(JavaVersion.VERSION_17, JavaVersion.VERSION_21))
}

tasks.jar {
    manifest {
        attributes("Main-Class" to "net.ornithemc.server_launcher.ServerLauncher")
    }
}

tasks.shadowJar {
    exclude("META-INF/MANIFEST.MF")
    relocate("com.google.gson", "net.ornithemc.flap.lib.gson") // use flap's shadowed gson because we know it supports record deserialization
}

tasks.downgradeJar {
    dependsOn(tasks.jar)
    dependsOn(tasks.shadowJar)
    inputFile.set(tasks.shadowJar.get().archiveFile)
}

tasks.shadeDowngradedApi {
    archiveFileName = "ServerLauncher.jar"
    destinationDirectory = file(System.getenv()["OUT_DIR"]!!)
    archiveClassifier = ""
}

tasks.assemble {
    dependsOn(tasks.shadeDowngradedApi)
}



