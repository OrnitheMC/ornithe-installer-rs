package net.ornithemc.server_launcher;

import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.jar.Attributes;
import java.util.jar.Manifest;

import com.google.gson.GsonBuilder;
import com.google.gson.annotations.SerializedName;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class ServerLauncher {
	private static final Logger log = LoggerFactory.getLogger(ServerLauncher.class);

    static {
        System.setProperty("org.slf4j.simpleLogger.logFile", "System.out");
    }

	public static void main(String[] args) {
		var processInfo = ProcessHandle.current().info();
		List<String> cmd = new ArrayList<>();
		cmd.add(processInfo.command().orElseThrow());
		var in = ServerLauncher.class.getResourceAsStream("/ornithe-args.json");
		var gson = new GsonBuilder().create();
		var arguments = new ArrayList<String>();
		processInfo.arguments().ifPresent(a -> Collections.addAll(arguments, a));
		arguments.removeAll(Arrays.asList(args));
		if (arguments.contains("-jar")) {
			int jarIndex = arguments.indexOf("-jar");
			var jar = arguments.get(jarIndex + 1);
			try (var fs = FileSystems.newFileSystem(Path.of(jar)); var mnIn = Files.newInputStream(fs.getPath("/META-INF/MANIFEST.MF"))) {
				var mn = new Manifest(mnIn);
				var attributes = mn.getMainAttributes();
				if (attributes.containsKey(Attributes.Name.CLASS_PATH)) {
					arguments.set(jarIndex, "-cp");
					var cp = new ArrayList<String>();
					Collections.addAll(cp, attributes.getValue(Attributes.Name.CLASS_PATH).split(" "));
					cp.add(jar);
					arguments.set(jarIndex + 1, String.join(File.pathSeparator, cp));
				}
			} catch (IOException e) {
				log.error("Failed to read launcher jar manifest:", e);
			}
		}
		var className = ServerLauncher.class.getName();
		if (in != null) {
			try (in;
				 var reader = new InputStreamReader(in)) {
				var ornitheArgs = gson.fromJson(reader, OrnitheArgs.class);
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
		cmd.addAll(arguments);
		Collections.addAll(cmd, args);
		//log.info("Starting: {}", String.join(" ", cmd));
		try {
			new ProcessBuilder(cmd).inheritIO().start().waitFor();
		} catch (IOException | InterruptedException e) {
			log.error("Error while starting server:", e);
		}
	}

	private record OrnitheArgs(@SerializedName("flap_jar") String flapJar,
                               @SerializedName("main_class") String mainClass,
							   @SerializedName("jvm_args") List<String> jvmArgs) {
	}
}
