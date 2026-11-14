import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class RunController {
    @GetMapping("/run")
    public String run(String value) throws Exception {
        return prepare(value);
    }

    private String prepare(String value) throws Exception {
        return execute(value);
    }

    private String execute(String value) throws Exception {
        Runtime.getRuntime().exec("echo " + value);
        return value;
    }

    private void orphan(String text) throws Exception {
        Runtime.getRuntime().exec(text);
    }
}
