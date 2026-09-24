<?php
namespace App\Http;
interface Action {}
trait Loggable {}
abstract class Base {}
#[\Attribute]
final class Route {}
class HomeController extends Base implements Action {
    use Loggable;
    public function index(): void {}
    public function show(): void {}
}
#[Route]
final class AboutController implements Action {
    public function __invoke(): void {}
}
