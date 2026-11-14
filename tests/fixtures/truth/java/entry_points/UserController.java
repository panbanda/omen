@RestController
@RequestMapping("/api")
public class UserController {
    @PreAuthorize("hasRole('ADMIN')")
    @GetMapping("/users/{id}")
    public String show(@PathVariable String id) {
        return id;
    }

    public static void main(String[] arguments) {}
}
