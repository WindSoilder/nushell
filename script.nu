#!/usr/bin/env nu

# def main [] {
#   module spam { export def foo [] { "foo" } }
#   overlay use spam
#   def bar [] { "bar" }
#   overlay hide spam
#   bar # Returns bar
# }


#!/usr/bin/env nu

#def main [] {
def something [] { "example" }
module spam2 {  }
overlay use spam2
overlay new spam3
def bar [] { "bar" }
overlay hide spam3
bar
#}
