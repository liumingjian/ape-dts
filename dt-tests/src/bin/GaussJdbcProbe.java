import java.io.BufferedReader;
import java.io.File;
import java.io.FileReader;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Properties;

public final class GaussJdbcProbe {
    public static void main(String[] args) throws Exception {
        Map<String, String> env = new LinkedHashMap<String, String>();
        load(env, "dt-tests/tests/.env");
        load(env, "dt-tests/tests/.env.local");

        String prefix = get(env, "GAUSSDB_PROBE_PREFIX", "gaussdb_pg_extractor");
        String baseUrl = require(env, prefix + "_without_auth_url");
        String username = require(env, prefix + "_username");
        String password = require(env, prefix + "_password");
        String candidates = get(env, "gaussdb_pg_candidate_hosts", "");

        System.out.println("prefix=" + prefix);
        System.out.println("base_url=" + redact(baseUrl));
        System.out.println("username=" + username);
        System.out.println("candidate_hosts=" + candidates);

        Class.forName("org.postgresql.Driver");

        boolean ok = false;
        for (String hostPort : candidateHostPorts(baseUrl, candidates)) {
            for (String jdbcUrl : toJdbcUrls(baseUrl, hostPort)) {
                try {
                    String info = probe(jdbcUrl, username, password);
                    ok = true;
                    System.out.println("OK " + redact(jdbcUrl) + " " + info);
                } catch (Exception e) {
                    System.out.println("ERR " + redact(jdbcUrl) + " " + describe(e));
                }
            }
        }

        if (!ok) {
            throw new IllegalStateException("all GaussDB JDBC probe attempts failed");
        }
    }

    private static void load(Map<String, String> env, String path) throws Exception {
        File file = new File(path);
        if (!file.exists()) {
            return;
        }
        BufferedReader reader = new BufferedReader(new FileReader(file));
        try {
            String line;
            while ((line = reader.readLine()) != null) {
                line = line.trim();
                if (line.isEmpty() || line.startsWith("#")) {
                    continue;
                }
                int idx = line.indexOf('=');
                if (idx <= 0) {
                    continue;
                }
                env.put(line.substring(0, idx).trim(), line.substring(idx + 1).trim());
            }
        } finally {
            reader.close();
        }
    }

    private static String probe(String jdbcUrl, String username, String password) throws Exception {
        Properties props = new Properties();
        props.setProperty("user", username);
        props.setProperty("password", password);
        props.setProperty("connectTimeout", "10");
        props.setProperty("socketTimeout", "10");

        Connection conn = DriverManager.getConnection(jdbcUrl, props);
        try {
            Statement stmt = conn.createStatement();
            try {
                ResultSet rs = stmt.executeQuery("SELECT current_user, current_database(), inet_server_addr()::text");
                try {
                    if (!rs.next()) {
                        return "no rows";
                    }
                    return "current_user=" + rs.getString(1)
                        + " current_database=" + rs.getString(2)
                        + " server_addr=" + rs.getString(3);
                } finally {
                    rs.close();
                }
            } finally {
                stmt.close();
            }
        } finally {
            conn.close();
        }
    }

    private static String[] candidateHostPorts(String baseUrl, String candidates) {
        if (candidates == null || candidates.trim().isEmpty()) {
            return new String[] { hostPortFromPostgresUrl(baseUrl) };
        }
        return candidates.split(",");
    }

    private static String[] toJdbcUrls(String postgresUrl, String hostPort) {
        Parsed parsed = parsePostgresUrl(postgresUrl);
        String hp = hostPort.trim();
        String base = "jdbc:postgresql://" + hp + parsed.path;
        String query = parsed.query.isEmpty() ? "" : "?" + parsed.query;
        return new String[] {
            base + query,
            base,
            base + "?ssl=false",
            base + "?ssl=true"
        };
    }

    private static String hostPortFromPostgresUrl(String postgresUrl) {
        Parsed parsed = parsePostgresUrl(postgresUrl);
        return parsed.hostPort;
    }

    private static Parsed parsePostgresUrl(String url) {
        String body = url.replaceFirst("^postgres://", "");
        int slash = body.indexOf('/');
        String hostPort = slash >= 0 ? body.substring(0, slash) : body;
        String rest = slash >= 0 ? body.substring(slash) : "/postgres";
        int query = rest.indexOf('?');
        String path = query >= 0 ? rest.substring(0, query) : rest;
        String queryPart = query >= 0 ? rest.substring(query + 1) : "";
        return new Parsed(hostPort, path, queryPart);
    }

    private static String require(Map<String, String> env, String key) {
        String value = env.get(key);
        if (value == null || value.isEmpty()) {
            throw new IllegalArgumentException("missing " + key);
        }
        return value;
    }

    private static String get(Map<String, String> env, String key, String fallback) {
        String value = env.get(key);
        return value == null ? fallback : value;
    }

    private static String redact(String url) {
        return url.replaceAll("//[^/@:]+:[^/@]+@", "//***:***@");
    }

    private static String describe(Exception err) {
        StringBuilder out = new StringBuilder();
        Throwable cur = err;
        while (cur != null) {
            out.append(cur.getClass().getName()).append(": ").append(cur.getMessage());
            if (cur instanceof SQLException) {
                SQLException sql = (SQLException) cur;
                out.append(" sqlState=").append(sql.getSQLState());
                out.append(" vendorCode=").append(sql.getErrorCode());
                SQLException next = sql.getNextException();
                if (next != null) {
                    out.append(" next=[")
                        .append(next.getClass().getName())
                        .append(": ")
                        .append(next.getMessage())
                        .append("]");
                }
            }
            cur = cur.getCause();
            if (cur != null) {
                out.append(" cause: ");
            }
        }
        return out.toString();
    }

    private static final class Parsed {
        final String hostPort;
        final String path;
        final String query;

        Parsed(String hostPort, String path, String query) {
            this.hostPort = hostPort;
            this.path = path;
            this.query = query;
        }
    }
}
