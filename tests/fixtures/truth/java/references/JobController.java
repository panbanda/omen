import java.io.IOException;
import java.util.concurrent.ExecutorService;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class JobController {
    private ExecutorService pool;
    private Library library;

    private void audit() {
        try {
            Runtime.getRuntime().exec("logger " + System.getenv("EVENT"));
        } catch (IOException error) {
            throw new IllegalStateException(error);
        }
    }

    @PostMapping("/jobs")
    public String createJob(String value) {
        pool.submit(() -> {
            try {
                Runtime.getRuntime().exec("sh -c " + value);
            } catch (IOException error) {
                throw new IllegalStateException(error);
            }
        });
        pool.submit(this::audit);
        return "ok";
    }

    @PostMapping("/shadow")
    public String shadowed(Auditor audit) {
        pool.submit(audit::audit);
        return "ok";
    }

    @PostMapping("/other")
    public String otherFile() {
        pool.submit(library::cleanup);
        return "ok";
    }

    @PostMapping("/text")
    public String textOnly() {
        // pool.submit(this::audit);
        return "this::audit";
    }
}
