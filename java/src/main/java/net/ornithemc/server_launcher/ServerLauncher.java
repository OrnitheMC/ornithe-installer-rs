package net.ornithemc.server_launcher;

import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.Reader;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.*;
import java.util.jar.Attributes;
import java.util.jar.Manifest;

import com.google.gson.GsonBuilder;
import com.google.gson.annotations.SerializedName;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class ServerLauncher {
	private static final Logger log = LoggerFactory.getLogger(ServerLauncher.class);

	public static void main(String[] args) {
		var processInfo = ProcessHandle.current().info();
		List<String> cmd = new ArrayList<>();
		cmd.add(processInfo.command().orElseThrow());
		var in = ServerLauncher.class.getResourceAsStream("/ornithe-args.json");
		var gson = new GsonBuilder().create();
		var arguments = new ArrayList<String>();
		processInfo.arguments().ifPresent(a -> Collections.addAll(arguments, a));
		arguments.removeAll(Arrays.asList(args));
		var cp = new ArrayList<String>();
		if (in != null) {
			try (in; var reader = new InputStreamReader(in)) {
				var ornitheArgs = gson.fromJson(reader, OrnitheArgs.class);
				if (arguments.contains("-jar")) {
					int jarIndex = arguments.indexOf("-jar");
					var jar = arguments.get(jarIndex + 1);
					try (var fs = FileSystems.newFileSystem(Path.of(jar)); var mnIn = Files.newInputStream(fs.getPath("/META-INF/MANIFEST.MF"))) {
						var mn = new Manifest(mnIn);
						var attributes = mn.getMainAttributes();
						if (attributes.containsKey(Attributes.Name.CLASS_PATH)) {
							arguments.set(jarIndex, "-cp");
							Collections.addAll(cp, attributes.getValue(Attributes.Name.CLASS_PATH).split(" "));
							cp.remove(ornitheArgs.flapJar);
							cp.add(ServerLauncher.class.getProtectionDomain().getCodeSource().getLocation().getPath());
							arguments.set(jarIndex + 1, String.join(File.pathSeparator, cp));
						}
					} catch (IOException e) {
						log.error("Failed to read launcher jar manifest:", e);
					}
				}
				var className = ServerLauncher.class.getName();

				cmd.add("-javaagent:" + ornitheArgs.flapJar);
				cmd.addAll(ornitheArgs.jvmArgs);
				var index = arguments.indexOf(className);
				if (index > -1) {
					arguments.set(index, ornitheArgs.mainClass);
				} else {
					arguments.add(ornitheArgs.mainClass);
				}
			} catch (IOException e) {
				log.error("Failed to read ornithe launch arguments:", e);
				return;
			}
		}
		try {
			var serverJar = stripShadedLibs(getServerJarPath(), cp);
			cmd.add("-Dfabric.gameJarPath=" + serverJar);
			cmd.add("-Dloader.gameJarPath=" + serverJar);
		} catch (IOException e) {
			log.error("Failed to transform server jar:", e);
		}
		cmd.addAll(arguments);
		Collections.addAll(cmd, args);
		log.debug("Starting: {}", String.join(" ", cmd));
		try {
			new ProcessBuilder(cmd).inheritIO().start().waitFor();
		} catch (IOException | InterruptedException e) {
			log.error("Error while starting server:", e);
		}
	}

	private static Path stripShadedLibs(String serverJar, List<String> cp) throws IOException {
		var serverPath = Path.of(serverJar);
		String serverHash;
		try {
			var bytes = MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(serverPath));
			StringBuilder sb = new StringBuilder(2 * bytes.length);
			var hexDigits = "0123456789abcdef".toCharArray();
			for (byte b : bytes) {
				sb.append(hexDigits[b >> 4 & 15]).append(hexDigits[b & 15]);
			}
			serverHash = sb.toString();
		} catch (NoSuchAlgorithmException e) {
			throw new IOException(e);
		}
		var transformedServerJar = Path.of(".ornithe", "transformedServerJars", serverHash + ".jar");
		if (!Files.exists(transformedServerJar)) {
			Files.createDirectories(transformedServerJar.getParent());
			List<String> libPaths = new ArrayList<>();
			for (var lib : cp) {
				if (!lib.startsWith("libraries")) continue;
				try (var fs = FileSystems.newFileSystem(Path.of(lib))) {
					for (var root : fs.getRootDirectories()) {
						try (var stream = Files.walk(root)) {
							libPaths.addAll(stream.filter(Files::isRegularFile).map(Path::toString).toList());
						}
					}
				}
			}
			Files.copy(serverPath, transformedServerJar);
			try (var fs = FileSystems.newFileSystem(transformedServerJar)) {
				for (String lib : libPaths) {
					var path = fs.getPath(lib);
					Files.deleteIfExists(path);
				}
			}
		}
		return transformedServerJar;
	}

	private static String getServerJarPath() throws IOException {
		Path propertiesFile = Path.of("fabric-server-launcher.properties");
		Properties properties = new Properties();
		if (!Files.exists(propertiesFile)) {
			propertiesFile = Path.of("quilt-server-launcher.properties");
		}

		if (Files.exists(propertiesFile)) {
			try (Reader reader = Files.newBufferedReader(propertiesFile)) {
				properties.load(reader);
			}
		}

		// Most popular Minecraft server hosting platforms do not allow
		// passing arbitrary arguments to the server .JAR. Meanwhile,
		// Mojang's default server filename is "server.jar" as of
		// a few versions... let's use this.
		if (!properties.containsKey("serverJar")) {
			properties.put("serverJar", "server.jar");
		}

		return (String) properties.get("serverJar");
	}

	private record OrnitheArgs(@SerializedName("flap_jar") String flapJar,
							   @SerializedName("main_class") String mainClass,
							   @SerializedName("jvm_args") List<String> jvmArgs) {
	}
}
