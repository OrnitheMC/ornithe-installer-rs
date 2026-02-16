plugins {
    java
    id("xyz.wagyourtail.jvmdowngrader") version "1.3.4"
}

java {
    targetCompatibility = JavaVersion.VERSION_21
    sourceCompatibility = JavaVersion.VERSION_21

    toolchain {
        languageVersion = JavaLanguageVersion.of(21)
        vendor = JvmVendorSpec.MICROSOFT
    }
}

base {
    archivesName = "ServerLauncher"
}

repositories {
    mavenCentral()
    maven("https://maven.ornithemc.net/releases")
}

dependencies {
    compileOnly("net.ornithemc:flap:0.0.1")
    compileOnly("org.apache.logging.log4j:log4j-core:2.19.0")
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

tasks.downgradeJar {
    dependsOn(tasks.jar)
    inputFile.set(tasks.jar.get().archiveFile)
}

tasks.shadeDowngradedApi {
    archiveFileName = "ServerLauncher.jar"
    destinationDirectory = file(System.getenv()["OUT_DIR"]!!)
    archiveClassifier = ""
}

tasks.assemble {
    dependsOn(tasks.shadeDowngradedApi)
}



