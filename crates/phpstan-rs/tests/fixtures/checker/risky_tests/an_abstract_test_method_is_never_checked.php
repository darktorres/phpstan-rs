<?php
        namespace PHPUnit\Framework {
            class TestCase {}
        }
        namespace {
            
            abstract class MyTest extends \PHPUnit\Framework\TestCase {
                abstract public function testSomething();
            }
        
        }
        