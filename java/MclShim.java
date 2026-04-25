import java.lang.reflect.Method;

public class MclShim {
    public static void main(String[] args) throws Exception {
        if (args.length == 0) {
            System.err.println("Usage: MclShim <mainClass> [args...]");
            System.exit(1);
        }

        String mainClass = args[0];
        String[] remaining = new String[args.length - 1];
        System.arraycopy(args, 1, remaining, 0, remaining.length);

        ClassLoader loader = ClassLoader.getSystemClassLoader();
        Class<?> clazz = loader.loadClass(mainClass);
        Method main = clazz.getMethod("main", String[].class);
        main.invoke(null, (Object) remaining);
    }
}
