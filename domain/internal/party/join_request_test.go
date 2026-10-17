package party

import (
	"testing"
	"time"
)

func TestJoinRequestRequiresCurrentLeaderConsentAndKeepsParty(t *testing.T) {
	h := NewHub()
	now := time.Unix(100, 0)
	leader := bindTest(t, h, 501, 1)
	app := bindTest(t, h, 502, 1)
	other := bindTest(t, h, 503, 1)
	p, e := h.Create(leader)
	if e != nil {
		t.Fatal(e)
	}
	id, e := h.RequestJoin(app, leader, now)
	if e != nil {
		t.Fatal(e)
	}
	if _, ok := h.Of(app); ok {
		t.Fatal("application joined before consent")
	}
	if _, e = h.Respond(app, id, true, now); e == nil {
		t.Fatal("applicant approved own request")
	}
	if _, e = h.Respond(other, id, true, now); e == nil {
		t.Fatal("foreign actor approved")
	}
	if again, e := h.RequestJoin(app, leader, now); e != nil || again != id {
		t.Fatal("duplicate request", again, e)
	}
	got, e := h.RespondFrom(leader, app.Actor, true, now)
	if e != nil {
		t.Fatal(e)
	}
	if got.ID != p.ID || got.Leader != leader.Actor || got.Seats != [4]uint16{leader.Actor, app.Actor, 0, 0} {
		t.Fatal("party replaced", got)
	}
	if _, e = h.Respond(leader, id, true, now); e == nil {
		t.Fatal("request replay committed")
	}
	accepted := h.Accepted(app)
	if len(accepted) != 1 || accepted[0].Peer != leader {
		t.Fatal("acceptance sent to wrong endpoint")
	}
}

func TestJoinRequestExpiryDeclineLeaderChangeAndCapacity(t *testing.T) {
	for _, scenario := range []string{"expired", "declined", "leader-leaves", "new-generation", "filled"} {
		t.Run(scenario, func(t *testing.T) {
			h := NewHub()
			now := time.Unix(100, 0)
			leader := bindTest(t, h, 501, 1)
			app := bindTest(t, h, 502, 1)
			if _, e := h.Create(leader); e != nil {
				t.Fatal(e)
			}
			id, e := h.RequestJoin(app, leader, now)
			if e != nil {
				t.Fatal(e)
			}
			switch scenario {
			case "expired":
				now = now.Add(31 * time.Second)
			case "declined":
				if _, e = h.Respond(leader, id, false, now); e != nil {
					t.Fatal(e)
				}
			case "leader-leaves":
				if _, _, e = h.Leave(leader); e != nil {
					t.Fatal(e)
				}
				if _, e = h.Create(leader); e != nil {
					t.Fatal(e)
				}
			case "new-generation":
				h.Unbind(app)
				app = bindTest(t, h, 502, 1)
			case "filled":
				for a := uint16(503); a <= 505; a++ {
					l := bindTest(t, h, a, 1)
					invite, e := h.Invite(leader, l, now)
					if e != nil {
						t.Fatal(e)
					}
					if _, e = h.Respond(l, invite, true, now); e != nil {
						t.Fatal(e)
					}
				}
			}
			if _, e = h.Respond(leader, id, true, now); e == nil {
				t.Fatal("invalid application admitted", scenario)
			}
			if _, ok := h.Of(app); ok {
				t.Fatal("failed request acquired membership")
			}
		})
	}
}
